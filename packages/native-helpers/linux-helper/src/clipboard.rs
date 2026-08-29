//! Clipboard access and the pasteText / getSelectedTextViaCopy flows.
//!
//! Uses the data-control protocols (`ext-data-control-v1`, with fallback to
//! `zwlr-data-control-unstable-v1`) via wl-clipboard-rs, which lets a
//! surfaceless client read and write the clipboard — supported by wlroots
//! compositors, KWin, and Mutter 48+.
//!
//! Simulating the actual paste/copy keystroke needs key injection (see
//! `wayland.rs`); when that's unavailable, pasteText degrades to
//! clipboard-only and getSelectedTextViaCopy reports no capture.

use std::io::Read;
use std::process::Stdio;
use std::time::Duration;

use tokio::io::AsyncWriteExt;
use wl_clipboard_rs::copy;
use wl_clipboard_rs::paste;

use crate::rpc::{GetSelectedTextViaCopyResult, PasteTextParams, SuccessMessageResult};
use crate::wayland::{KeyInjector, KEY_C, KEY_V, MOD_CTRL};

/// How long to wait for the focused app to answer the injected copy chord.
/// Mirrors the other helpers' 300ms clipboard poll budget.
const COPY_POLL_TOTAL: Duration = Duration::from_millis(400);
const COPY_POLL_STEP: Duration = Duration::from_millis(25);

/// Delay before restoring the clipboard after an injected paste, giving the
/// focused app time to read the offer we still serve.
const PASTE_RESTORE_DELAY: Duration = Duration::from_millis(500);

/// Outcome of the primary (data-control) clipboard path.
enum DataControl<T> {
    Done(T),
    /// The compositor lacks ext-data-control/wlr-data-control (e.g. Mutter
    /// before GNOME 48) — fall back to the wl-clipboard CLI tools, which
    /// handle such compositors with a transient-surface workaround.
    Unsupported,
}

fn read_clipboard_text() -> Result<DataControl<Option<String>>, String> {
    match paste::get_contents(
        paste::ClipboardType::Regular,
        paste::Seat::Unspecified,
        paste::MimeType::Text,
    ) {
        Ok((mut pipe, _mime)) => {
            let mut contents = String::new();
            pipe.read_to_string(&mut contents)
                .map_err(|e| format!("clipboard read failed: {e}"))?;
            Ok(DataControl::Done(Some(contents)))
        }
        Err(paste::Error::NoMimeType) | Err(paste::Error::ClipboardEmpty) => {
            Ok(DataControl::Done(None))
        }
        Err(paste::Error::MissingProtocol { .. }) => Ok(DataControl::Unsupported),
        Err(e) => Err(format!("clipboard read failed: {e}")),
    }
}

fn write_clipboard_text(text: String) -> Result<DataControl<()>, String> {
    // Non-foreground mode: wl-clipboard-rs spawns a thread that keeps serving
    // the offer after this returns, which is exactly right for a long-lived
    // helper process.
    match copy::Options::new().copy(
        copy::Source::Bytes(text.into_bytes().into()),
        copy::MimeType::Text,
    ) {
        Ok(()) => Ok(DataControl::Done(())),
        Err(copy::Error::MissingProtocol { .. }) => Ok(DataControl::Unsupported),
        Err(e) => Err(format!("clipboard write failed: {e}")),
    }
}

async fn get_text_via_wl_paste() -> Result<Option<String>, String> {
    let output = tokio::process::Command::new("wl-paste")
        .args(["--no-newline", "--type", "text"])
        .output()
        .await
        .map_err(|e| format!("wl-paste unavailable: {e}"))?;
    if output.status.success() {
        Ok(Some(String::from_utf8_lossy(&output.stdout).into_owned()))
    } else {
        // wl-paste exits 1 both for "nothing is copied" and real errors;
        // treat as empty, which is the harmless interpretation.
        Ok(None)
    }
}

async fn set_text_via_wl_copy(text: String) -> Result<(), String> {
    let mut child = tokio::process::Command::new("wl-copy")
        .arg("--type")
        .arg("text/plain;charset=utf-8")
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|e| format!("wl-copy unavailable: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(text.as_bytes())
            .await
            .map_err(|e| format!("wl-copy write failed: {e}"))?;
    }
    let status = child
        .wait()
        .await
        .map_err(|e| format!("wl-copy failed: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("wl-copy exited with {status}"))
    }
}

pub async fn get_text() -> Result<Option<String>, String> {
    let primary = tokio::task::spawn_blocking(read_clipboard_text)
        .await
        .map_err(|e| format!("clipboard task panicked: {e}"))??;
    match primary {
        DataControl::Done(text) => Ok(text),
        DataControl::Unsupported => get_text_via_wl_paste().await,
    }
}

pub async fn set_text(text: String) -> Result<(), String> {
    let primary = tokio::task::spawn_blocking({
        let text = text.clone();
        move || write_clipboard_text(text)
    })
    .await
    .map_err(|e| format!("clipboard task panicked: {e}"))??;
    match primary {
        DataControl::Done(()) => Ok(()),
        DataControl::Unsupported => set_text_via_wl_copy(text).await,
    }
}

/// pasteText: put the transcript on the clipboard, then best-effort inject
/// Ctrl+V. `preserveClipboard` restores the previous text contents afterwards
/// (only meaningful when injection worked; otherwise the transcript must stay
/// on the clipboard for the user to paste manually).
pub async fn paste_text(
    params: PasteTextParams,
    injector: Option<KeyInjector>,
) -> Result<SuccessMessageResult, String> {
    let preserve = params.preserve_clipboard.unwrap_or(false);

    let saved = if preserve {
        match get_text().await {
            Ok(text) => text,
            Err(e) => {
                eprintln!("[clipboard] could not save clipboard for preserve: {e}");
                None
            }
        }
    } else {
        None
    };

    set_text(params.transcript).await?;

    let Some(injector) = injector else {
        return Ok(SuccessMessageResult {
            success: true,
            message: Some(
                "Transcript copied to clipboard; automatic paste is not supported by this compositor — press Ctrl+V to insert it".into(),
            ),
        });
    };

    let injected =
        tokio::task::spawn_blocking(move || injector.inject_chord_blocking(MOD_CTRL, KEY_V))
            .await
            .unwrap_or(false);

    if injected {
        if let Some(saved) = saved {
            tokio::time::sleep(PASTE_RESTORE_DELAY).await;
            if let Err(e) = set_text(saved).await {
                eprintln!("[clipboard] failed to restore clipboard: {e}");
            }
        }
        Ok(SuccessMessageResult {
            success: true,
            message: Some("Pasted".into()),
        })
    } else {
        Ok(SuccessMessageResult {
            success: true,
            message: Some(
                "Transcript copied to clipboard, but the paste keystroke could not be injected"
                    .into(),
            ),
        })
    }
}

/// getSelectedTextViaCopy: inject Ctrl+C, poll the clipboard for a change,
/// restore the previous text contents. Only text can be saved/restored via
/// data-control here — a limitation over the macOS/Windows helpers which
/// restore all formats.
pub async fn get_selected_text_via_copy(
    injector: Option<KeyInjector>,
) -> GetSelectedTextViaCopyResult {
    let Some(injector) = injector else {
        return GetSelectedTextViaCopyResult {
            selected_text: None,
            clipboard_changed: false,
            message: Some(
                "Copy capture unavailable: compositor does not support key injection".into(),
            ),
        };
    };

    let before = match get_text().await {
        Ok(text) => text,
        Err(e) => {
            return GetSelectedTextViaCopyResult {
                selected_text: None,
                clipboard_changed: false,
                message: Some(e),
            };
        }
    };

    let injected =
        tokio::task::spawn_blocking(move || injector.inject_chord_blocking(MOD_CTRL, KEY_C))
            .await
            .unwrap_or(false);
    if !injected {
        return GetSelectedTextViaCopyResult {
            selected_text: None,
            clipboard_changed: false,
            message: Some("Failed to inject copy keystroke".into()),
        };
    }

    let mut after: Option<String> = None;
    let mut waited = Duration::ZERO;
    while waited < COPY_POLL_TOTAL {
        tokio::time::sleep(COPY_POLL_STEP).await;
        waited += COPY_POLL_STEP;
        match get_text().await {
            Ok(current) if current != before => {
                after = current;
                break;
            }
            _ => {}
        }
    }

    let clipboard_changed = after.is_some();
    let mut message = None;

    // Restore what was on the clipboard before the injected copy.
    if clipboard_changed {
        if let Some(before) = before {
            if let Err(e) = set_text(before).await {
                message = Some(format!("Clipboard restore failed: {e}"));
            }
        }
    }

    GetSelectedTextViaCopyResult {
        selected_text: after,
        clipboard_changed,
        message,
    }
}
