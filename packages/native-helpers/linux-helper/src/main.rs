//! Amical Linux helper (Wayland).
//!
//! A standard UNIX filter: JSON-RPC 2.0 requests arrive on stdin (one JSON
//! object per line), responses and unsolicited events leave on stdout (one
//! JSON object per line). Diagnostics go to stderr, which the desktop logs.
//!
//! See LINUX_HELPER_SPEC.md and packages/types/src/schemas for the protocol.

mod accessibility;
mod audio;
mod clipboard;
mod input;
mod keycodes;
mod rpc;
#[cfg(feature = "uinput")]
mod shortcut_release;
mod shortcuts;
mod wayland;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

use rpc::{RpcRequest, RpcResponse};

struct Helper {
    out_tx: mpsc::UnboundedSender<String>,
    injector: Option<input::KeyInjector>,
    clipboard_lock: tokio::sync::Mutex<()>,
    shortcuts: Option<Arc<shortcuts::ShortcutsService>>,
    audio: audio::AudioService,
    /// Accepted for protocol parity; a Wayland helper cannot intercept the
    /// Enter key of the focused app, so this is state without effect.
    draft_enter_capture: AtomicBool,
}

#[tokio::main]
async fn main() {
    if std::env::args().any(|arg| arg == "--print-build-input-method") {
        println!("{}", linux_helper::compile_time_method().as_str());
        return;
    }

    // Single writer task serializes all stdout lines (RPC responses and
    // unsolicited events) so they never interleave.
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<String>();
    let writer = tokio::spawn(async move {
        let mut stdout = tokio::io::stdout();
        while let Some(line) = out_rx.recv().await {
            let mut buf = line.into_bytes();
            buf.push(b'\n');
            if stdout.write_all(&buf).await.is_err() {
                break;
            }
            let _ = stdout.flush().await;
        }
    });

    let injector = match input::KeyInjector::new() {
        Ok(injector) => {
            eprintln!(
                "[main] {} key injection ready",
                linux_helper::current_method().as_str()
            );
            Some(injector)
        }
        Err(e) => {
            eprintln!("[main] automatic paste unavailable: {e}");
            None
        }
    };

    let shortcuts = match shortcuts::ShortcutsService::new(out_tx.clone()).await {
        Ok(service) => Some(service),
        Err(e) => {
            eprintln!("[main] global shortcuts unavailable: {e}");
            None
        }
    };

    let helper = Arc::new(Helper {
        out_tx: out_tx.clone(),
        injector,
        clipboard_lock: tokio::sync::Mutex::new(()),
        shortcuts,
        audio: audio::AudioService::new(),
        draft_enter_capture: AtomicBool::new(false),
    });

    eprintln!(
        "[main] linux-helper ready (input method: {})",
        linux_helper::current_method().as_str()
    );

    let stdin = BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();
    while let Ok(Some(line)) = lines.next_line().await {
        if line.trim().is_empty() {
            continue;
        }
        let helper = helper.clone();
        tokio::spawn(async move {
            let response = match serde_json::from_str::<RpcRequest>(&line) {
                Ok(request) => handle_request(&helper, request).await,
                Err(e) => {
                    eprintln!("[main] unparseable request: {e}");
                    RpcResponse::failure("", rpc::PARSE_ERROR, format!("Parse error: {e}"))
                }
            };
            if let Ok(line) = serde_json::to_string(&response) {
                let _ = helper.out_tx.send(line);
            }
        });
    }

    // stdin closed: the desktop is gone, shut down.
    eprintln!("[main] stdin closed, exiting");
    drop(helper);
    drop(out_tx);
    let _ = writer.await;
}

fn parse_params<T: serde::de::DeserializeOwned>(request: &RpcRequest) -> Result<T, RpcResponse> {
    let params = request.params.clone().unwrap_or(serde_json::Value::Null);
    serde_json::from_value(params).map_err(|e| {
        RpcResponse::failure(
            &request.id,
            rpc::INVALID_PARAMS,
            format!("Invalid params for {}: {e}", request.method),
        )
    })
}

async fn handle_request(helper: &Helper, request: RpcRequest) -> RpcResponse {
    let id = request.id.clone();
    match request.method.as_str() {
        "getAccessibilityContext" => {
            RpcResponse::success(&id, accessibility::get_accessibility_context())
        }
        "getAccessibilityStatus" => {
            RpcResponse::success(&id, accessibility::get_accessibility_status())
        }
        "requestAccessibilityPermission" => {
            RpcResponse::success(&id, accessibility::request_accessibility_permission())
        }
        "getAccessibilityTreeDetails" => {
            RpcResponse::success(&id, accessibility::get_accessibility_tree_details())
        }

        "pasteText" => match parse_params::<rpc::PasteTextParams>(&request) {
            Ok(params) => {
                let _guard = helper.clipboard_lock.lock().await;
                match clipboard::paste_text(params, helper.injector.clone()).await {
                    Ok(result) => RpcResponse::success(&id, result),
                    Err(e) => RpcResponse::failure(&id, rpc::INTERNAL_ERROR, e),
                }
            }
            Err(response) => response,
        },

        "getSelectedTextViaCopy" => {
            let _guard = helper.clipboard_lock.lock().await;
            let result = clipboard::get_selected_text_via_copy(helper.injector.clone()).await;
            RpcResponse::success(&id, result)
        }

        "startRecording" => match parse_params::<rpc::StartRecordingParams>(&request) {
            Ok(params) => RpcResponse::success(&id, helper.audio.start_recording(params).await),
            Err(response) => response,
        },

        "stopRecording" => match parse_params::<rpc::StopRecordingParams>(&request) {
            Ok(params) => RpcResponse::success(&id, helper.audio.stop_recording(params).await),
            Err(response) => response,
        },

        "setShortcuts" => match parse_params::<rpc::SetShortcutsParams>(&request) {
            Ok(params) => {
                let success = match &helper.shortcuts {
                    Some(service) => match service.set_shortcuts(params).await {
                        Ok(()) => true,
                        Err(e) => {
                            eprintln!("[main] setShortcuts failed: {e}");
                            false
                        }
                    },
                    None => false,
                };
                RpcResponse::success(&id, rpc::SuccessResult { success })
            }
            Err(response) => response,
        },

        "setDraftEnterCapture" => match parse_params::<rpc::SetDraftEnterCaptureParams>(&request) {
            Ok(params) => {
                helper
                    .draft_enter_capture
                    .store(params.enabled, Ordering::Relaxed);
                RpcResponse::success(&id, rpc::SuccessResult { success: true })
            }
            Err(response) => response,
        },

        "setAllowInjectedKeys" => match parse_params::<rpc::SetAllowInjectedKeysParams>(&request) {
            // Windows-specific behavior; accept and no-op like the macOS helper.
            Ok(_) => RpcResponse::success(&id, rpc::SuccessResult { success: true }),
            Err(response) => response,
        },

        "recheckPressedKeys" => match parse_params::<rpc::RecheckPressedKeysParams>(&request) {
            Ok(params) => {
                let stale = match &helper.shortcuts {
                    Some(service) => service.stale_keys(&params.pressed_key_codes).await,
                    // No portal: we have no key truth, so every reported key
                    // is unverifiable; claim none are stale to avoid fighting
                    // the desktop's own state.
                    None => Vec::new(),
                };
                RpcResponse::success(
                    &id,
                    rpc::RecheckPressedKeysResult {
                        stale_key_codes: stale,
                    },
                )
            }
            Err(response) => response,
        },

        unknown => RpcResponse::failure(
            &id,
            rpc::METHOD_NOT_FOUND,
            format!("Method not found: {unknown}"),
        ),
    }
}
