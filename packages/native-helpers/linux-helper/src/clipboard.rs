//! Clipboard access and the pasteText / getSelectedTextViaCopy flows.
//!
//! Three clipboard backends, tried in this order:
//!
//! 1. The Amical GNOME Shell extension, when it is running: reads and writes
//!    happen inside the shell, so the focused application keeps focus.
//! 2. The data-control protocols (`ext-data-control-v1`, falling back to
//!    `zwlr-data-control-unstable-v1`) via wl-clipboard-rs, which let a
//!    surfaceless client use the clipboard: wlroots compositors and KWin.
//!    Mutter does not advertise them to ordinary clients.
//! 3. The `wl-copy` / `wl-paste` CLI tools. Without data-control they open a
//!    tiny transient window to obtain keyboard focus for every call, so the
//!    target application loses and regains focus once per operation. This is
//!    the "icon flash" on GNOME without the extension; the flow below keeps
//!    the number of such calls minimal and waits for focus to settle before
//!    injecting the paste chord.
//!
//! Simulating the actual paste/copy keystroke needs key injection (see
//! `input.rs`); when that's unavailable, pasteText degrades to
//! clipboard-only and getSelectedTextViaCopy reports no capture.

use std::io::Read;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use linux_helper::gnome_shell::GnomeShell;
use tokio::io::AsyncWriteExt;
use wl_clipboard_rs::copy;
use wl_clipboard_rs::paste;

use crate::input::KeyInjector;
use crate::rpc::{GetSelectedTextViaCopyResult, PasteTextParams, SuccessMessageResult};
use crate::wayland::{KEY_C, KEY_V, MOD_CTRL};

const KEY_INSERT: u32 = 110;
const MOD_SHIFT: u32 = 1;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Selection {
    Regular,
    Primary,
}

/// How long to wait for the focused app to answer the injected copy chord.
/// Mirrors the other helpers' 300ms clipboard poll budget.
const COPY_POLL_TOTAL: Duration = Duration::from_millis(400);
const COPY_POLL_STEP: Duration = Duration::from_millis(25);

/// Delay before restoring the clipboard after an injected paste, giving the
/// focused app time to read the offer we still serve.
const PASTE_RESTORE_DELAY: Duration = Duration::from_millis(500);

/// After a focus-stealing CLI clipboard call, Mutter hands focus back to the
/// target window asynchronously (measured 25–215 ms on GNOME 50). Wait this
/// long before injecting so the chord reaches the target, not the vanishing
/// transient window.
const FOCUS_SETTLE_DELAY: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardBackend {
    GnomeExtension,
    DataControl,
    WlClipboardCli,
}

impl ClipboardBackend {
    pub const fn name(self) -> &'static str {
        match self {
            Self::GnomeExtension => "gnome-extension",
            Self::DataControl => "data-control",
            Self::WlClipboardCli => "wl-clipboard-cli",
        }
    }

    pub const fn steals_focus(self) -> bool {
        matches!(self, Self::WlClipboardCli)
    }
}

/// Outcome of the primary (data-control) clipboard path.
enum DataControl<T> {
    Done(T),
    /// The compositor lacks ext-data-control/wlr-data-control (e.g. Mutter)
    /// — fall back to the wl-clipboard CLI tools, which handle such
    /// compositors with a transient-surface workaround.
    Unsupported,
}

fn read_clipboard_text(selection: Selection) -> Result<DataControl<Option<String>>, String> {
    match paste::get_contents(
        match selection {
            Selection::Regular => paste::ClipboardType::Regular,
            Selection::Primary => paste::ClipboardType::Primary,
        },
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

fn write_clipboard_text(text: String, selection: Selection) -> Result<DataControl<()>, String> {
    // Non-foreground mode: wl-clipboard-rs spawns a thread that keeps serving
    // the offer after this returns, which is exactly right for a long-lived
    // helper process.
    let mut options = copy::Options::new();
    options.clipboard(match selection {
        Selection::Regular => copy::ClipboardType::Regular,
        Selection::Primary => copy::ClipboardType::Primary,
    });
    match options.copy(
        copy::Source::Bytes(text.into_bytes().into()),
        copy::MimeType::Text,
    ) {
        Ok(()) => Ok(DataControl::Done(())),
        Err(copy::Error::MissingProtocol { .. }) => Ok(DataControl::Unsupported),
        Err(e) => Err(format!("clipboard write failed: {e}")),
    }
}

async fn get_text_via_wl_paste(selection: Selection) -> Result<Option<String>, String> {
    let mut command = tokio::process::Command::new("wl-paste");
    command.args(["--no-newline", "--type", "text"]);
    if selection == Selection::Primary {
        command.arg("--primary");
    }
    let output = command
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

async fn set_text_via_wl_copy(text: String, selection: Selection) -> Result<(), String> {
    let mut command = tokio::process::Command::new("wl-copy");
    command.args(["--type", "text/plain;charset=utf-8"]);
    if selection == Selection::Primary {
        command.arg("--primary");
    }
    let mut child = command
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

/// Whether the compositor offers data-control to this client. Probed once:
/// the answer does not change while the session runs.
fn data_control_supported() -> bool {
    static SUPPORTED: OnceLock<bool> = OnceLock::new();
    *SUPPORTED.get_or_init(|| {
        !matches!(
            read_clipboard_text(Selection::Regular),
            Ok(DataControl::Unsupported)
        )
    })
}

/// Shared clipboard access with backend selection. Cheap to clone.
#[derive(Clone)]
pub struct Clipboard {
    shell: Option<Arc<GnomeShell>>,
    /// Set when the focus-stealing CLI path was used for the latest write.
    focus_disturbed: Arc<AtomicBool>,
}

impl Clipboard {
    pub fn new(shell: Option<Arc<GnomeShell>>) -> Self {
        Self {
            shell,
            focus_disturbed: Arc::new(AtomicBool::new(false)),
        }
    }

    /// The backend the next operation will use.
    pub async fn backend(&self) -> ClipboardBackend {
        if let Some(shell) = &self.shell {
            if shell.available().await {
                return ClipboardBackend::GnomeExtension;
            }
        }
        let supported = tokio::task::spawn_blocking(data_control_supported)
            .await
            .unwrap_or(false);
        if supported {
            ClipboardBackend::DataControl
        } else {
            ClipboardBackend::WlClipboardCli
        }
    }

    async fn get_selection(&self, selection: Selection) -> Result<Option<String>, String> {
        match self.backend().await {
            ClipboardBackend::GnomeExtension => {
                self.shell
                    .as_ref()
                    .expect("extension backend implies a shell client")
                    .get_clipboard_text(selection == Selection::Primary)
                    .await
            }
            ClipboardBackend::DataControl => {
                match tokio::task::spawn_blocking(move || read_clipboard_text(selection))
                    .await
                    .map_err(|e| format!("clipboard task panicked: {e}"))??
                {
                    DataControl::Done(text) => Ok(text),
                    DataControl::Unsupported => get_text_via_wl_paste(selection).await,
                }
            }
            ClipboardBackend::WlClipboardCli => {
                self.focus_disturbed.store(true, Ordering::Relaxed);
                get_text_via_wl_paste(selection).await
            }
        }
    }

    async fn set_selection(&self, text: String, selection: Selection) -> Result<(), String> {
        match self.backend().await {
            ClipboardBackend::GnomeExtension => {
                self.shell
                    .as_ref()
                    .expect("extension backend implies a shell client")
                    .set_clipboard_text(&text, selection == Selection::Primary)
                    .await
            }
            ClipboardBackend::DataControl => {
                let primary = tokio::task::spawn_blocking({
                    let text = text.clone();
                    move || write_clipboard_text(text, selection)
                })
                .await
                .map_err(|e| format!("clipboard task panicked: {e}"))??;
                match primary {
                    DataControl::Done(()) => Ok(()),
                    DataControl::Unsupported => set_text_via_wl_copy(text, selection).await,
                }
            }
            ClipboardBackend::WlClipboardCli => {
                self.focus_disturbed.store(true, Ordering::Relaxed);
                set_text_via_wl_copy(text, selection).await
            }
        }
    }

    pub async fn get_text(&self) -> Result<Option<String>, String> {
        self.get_selection(Selection::Regular).await
    }

    pub async fn set_text(&self, text: String) -> Result<(), String> {
        self.set_selection(text, Selection::Regular).await
    }
}

/// pasteText: put the transcript on the clipboard, then best-effort inject
/// Ctrl+V (Wayland) or Shift+Insert (uinput, GNOME extension). For
/// Shift+Insert, supply both selections because terminals/toolkits differ on
/// which selection they read. `preserveClipboard` restores the previous text
/// contents afterwards (only meaningful when injection worked; otherwise the
/// transcript must stay on the clipboard for the user to paste manually).
pub async fn paste_text(
    params: PasteTextParams,
    clipboard: Clipboard,
    injector: Option<KeyInjector>,
) -> Result<SuccessMessageResult, String> {
    paste_text_on(
        params,
        &SystemPasteHost {
            clipboard,
            injector,
        },
    )
    .await
}

// Keep the delivery transaction testable without a live desktop or sending
// keystrokes to the developer's focused window.
trait PasteHost {
    async fn available(&self) -> bool;
    fn primary_paste(&self) -> bool;
    fn backend_name(&self) -> &'static str;
    async fn read(&self, selection: Selection) -> Result<Option<String>, String>;
    async fn write(&self, text: String, selection: Selection) -> Result<(), String>;
    /// Give the target window time to regain focus after a focus-stealing
    /// clipboard call; a no-op for backends that keep focus where it is.
    async fn settle_focus(&self);
    async fn inject(&self, mods: u32, key: u32) -> Result<(), String>;
    async fn restore_delay(&self);
}

#[cfg(test)]
mod paste_tests {
    use super::*;
    use std::sync::Mutex;

    struct FakeHost {
        contents: Mutex<[Option<String>; 2]>,
        available: bool,
        primary: bool,
        inject_ok: bool,
        primary_write_fails: bool,
        copy_during_paste: bool,
        chords: Mutex<Vec<(u32, u32)>>,
        settled_before_inject: Mutex<Vec<bool>>,
        settled: Mutex<bool>,
    }

    impl Default for FakeHost {
        fn default() -> Self {
            Self {
                contents: Mutex::new([
                    Some("previous clipboard".into()),
                    Some("previous selection".into()),
                ]),
                available: true,
                primary: true,
                inject_ok: true,
                primary_write_fails: false,
                copy_during_paste: false,
                chords: Mutex::new(Vec::new()),
                settled_before_inject: Mutex::new(Vec::new()),
                settled: Mutex::new(false),
            }
        }
    }

    impl PasteHost for FakeHost {
        async fn available(&self) -> bool {
            self.available
        }
        fn primary_paste(&self) -> bool {
            self.primary
        }
        fn backend_name(&self) -> &'static str {
            "fake"
        }
        async fn read(&self, selection: Selection) -> Result<Option<String>, String> {
            Ok(self.contents.lock().unwrap()[selection as usize].clone())
        }
        async fn write(&self, text: String, selection: Selection) -> Result<(), String> {
            if self.primary_write_fails && selection == Selection::Primary {
                return Err("primary unavailable".into());
            }
            self.contents.lock().unwrap()[selection as usize] = Some(text);
            *self.settled.lock().unwrap() = false;
            Ok(())
        }
        async fn settle_focus(&self) {
            *self.settled.lock().unwrap() = true;
        }
        async fn inject(&self, mods: u32, key: u32) -> Result<(), String> {
            // The right text must be offered before we send any key events.
            let contents = self.contents.lock().unwrap();
            assert_eq!(contents[0].as_deref(), Some("dictated text"));
            if self.primary {
                assert_eq!(contents[1].as_deref(), Some("dictated text"));
            }
            self.chords.lock().unwrap().push((mods, key));
            self.settled_before_inject
                .lock()
                .unwrap()
                .push(*self.settled.lock().unwrap());
            if self.inject_ok {
                Ok(())
            } else {
                Err("keys held".into())
            }
        }
        async fn restore_delay(&self) {
            if self.copy_during_paste {
                self.contents.lock().unwrap()[0] = Some("new user copy".into());
            }
        }
    }

    fn params(preserve: bool) -> PasteTextParams {
        PasteTextParams {
            transcript: "dictated text".into(),
            preserve_clipboard: Some(preserve),
        }
    }

    #[tokio::test]
    async fn uinput_paste_supplies_and_preserves_both_selections() {
        let host = FakeHost::default();
        assert!(paste_text_on(params(true), &host).await.unwrap().success);
        assert_eq!(*host.chords.lock().unwrap(), [(MOD_SHIFT, KEY_INSERT)]);
        assert_eq!(
            *host.contents.lock().unwrap(),
            [
                Some("previous clipboard".into()),
                Some("previous selection".into())
            ]
        );
    }

    #[tokio::test]
    async fn focus_is_settled_after_the_last_write_and_before_the_chord() {
        let host = FakeHost::default();
        assert!(paste_text_on(params(true), &host).await.unwrap().success);
        assert_eq!(*host.settled_before_inject.lock().unwrap(), [true]);
    }

    #[tokio::test]
    async fn default_wayland_paste_keeps_ctrl_v_and_primary_untouched() {
        let host = FakeHost {
            primary: false,
            ..Default::default()
        };
        assert!(paste_text_on(params(false), &host).await.unwrap().success);
        assert_eq!(*host.chords.lock().unwrap(), [(MOD_CTRL, KEY_V)]);
        assert_eq!(
            host.contents.lock().unwrap()[1].as_deref(),
            Some("previous selection")
        );
    }

    #[tokio::test]
    async fn fallback_never_claims_paste_or_restores_away_transcript() {
        for host in [
            FakeHost {
                available: false,
                primary: false,
                ..Default::default()
            },
            FakeHost {
                inject_ok: false,
                ..Default::default()
            },
            FakeHost {
                primary_write_fails: true,
                ..Default::default()
            },
        ] {
            let result = paste_text_on(params(true), &host).await.unwrap();
            assert!(!result.success);
            assert!(result.message.unwrap().contains("clipboard"));
            assert_eq!(
                host.contents.lock().unwrap()[0].as_deref(),
                Some("dictated text")
            );
            if !host.available || host.primary_write_fails {
                assert!(host.chords.lock().unwrap().is_empty());
            }
        }
    }

    #[tokio::test]
    async fn injection_failure_reason_reaches_the_user() {
        let host = FakeHost {
            inject_ok: false,
            ..Default::default()
        };
        let result = paste_text_on(params(false), &host).await.unwrap();
        assert!(!result.success);
        assert!(result.message.unwrap().contains("keys held"));
    }

    #[tokio::test]
    async fn restore_does_not_overwrite_new_user_copy() {
        let host = FakeHost {
            copy_during_paste: true,
            ..Default::default()
        };
        assert!(paste_text_on(params(true), &host).await.unwrap().success);
        assert_eq!(
            host.contents.lock().unwrap()[0].as_deref(),
            Some("new user copy")
        );
    }
}

struct SystemPasteHost {
    clipboard: Clipboard,
    injector: Option<KeyInjector>,
}

impl PasteHost for SystemPasteHost {
    async fn available(&self) -> bool {
        match &self.injector {
            Some(injector) => injector.available().await,
            None => false,
        }
    }
    fn primary_paste(&self) -> bool {
        self.injector
            .as_ref()
            .is_some_and(KeyInjector::uses_primary_paste)
    }
    fn backend_name(&self) -> &'static str {
        self.injector
            .as_ref()
            .map_or("none", KeyInjector::backend_name)
    }
    async fn read(&self, selection: Selection) -> Result<Option<String>, String> {
        self.clipboard.get_selection(selection).await
    }
    async fn write(&self, text: String, selection: Selection) -> Result<(), String> {
        self.clipboard.set_selection(text, selection).await
    }
    async fn settle_focus(&self) {
        if self
            .clipboard
            .focus_disturbed
            .swap(false, Ordering::Relaxed)
        {
            tokio::time::sleep(FOCUS_SETTLE_DELAY).await;
        }
    }
    async fn inject(&self, mods: u32, key: u32) -> Result<(), String> {
        match &self.injector {
            Some(injector) => injector.inject_chord(mods, key).await,
            None => Err("no key injection backend".into()),
        }
    }
    async fn restore_delay(&self) {
        tokio::time::sleep(PASTE_RESTORE_DELAY).await;
    }
}

async fn paste_text_on(
    params: PasteTextParams,
    host: &impl PasteHost,
) -> Result<SuccessMessageResult, String> {
    let started = Instant::now();
    let ms = |since: Instant| since.elapsed().as_millis();
    let preserve = params.preserve_clipboard.unwrap_or(false);
    let primary_paste = host.primary_paste();

    let saved = if preserve {
        match host.read(Selection::Regular).await {
            Ok(text) => text,
            Err(e) => {
                eprintln!("[clipboard] could not save clipboard for preserve: {e}");
                None
            }
        }
    } else {
        None
    };

    let saved_primary = if preserve && primary_paste {
        host.read(Selection::Primary).await.unwrap_or_else(|e| {
            eprintln!("[clipboard] could not save primary selection: {e}");
            None
        })
    } else {
        None
    };
    if preserve {
        eprintln!(
            "[clipboard] pasteText t+{}ms: previous contents saved",
            ms(started)
        );
    }

    let transcript = params.transcript;
    host.write(transcript.clone(), Selection::Regular).await?;

    if !host.available().await {
        eprintln!(
            "[clipboard] pasteText t+{}ms: transcript on clipboard, no injection ({})",
            ms(started),
            host.backend_name()
        );
        return Ok(SuccessMessageResult {
            success: false,
            message: Some(
                "Automatic paste is unavailable. Your transcript is on the clipboard; use your application's Paste command to insert it.".into(),
            ),
        });
    }

    if primary_paste {
        if let Err(e) = host.write(transcript.clone(), Selection::Primary).await {
            eprintln!("[clipboard] could not prepare primary selection: {e}");
            // Do not inject Shift+Insert with stale primary contents, or try a
            // second chord blindly: it could insert unrelated/duplicate text.
            return Ok(SuccessMessageResult {
                success: false,
                message: Some("Automatic paste could not prepare the text. Your transcript is on the clipboard; use your application's Paste command.".into()),
            });
        }
    }
    eprintln!(
        "[clipboard] pasteText t+{}ms: transcript offered",
        ms(started)
    );

    host.settle_focus().await;
    let (mods, key) = if primary_paste {
        (MOD_SHIFT, KEY_INSERT)
    } else {
        (MOD_CTRL, KEY_V)
    };
    let injected_at = Instant::now();
    let injected = host.inject(mods, key).await;
    eprintln!(
        "[clipboard] pasteText t+{}ms: chord {} via {} (inject took {}ms)",
        ms(started),
        match &injected {
            Ok(()) => "sent".to_string(),
            Err(e) => format!("failed: {e}"),
        },
        host.backend_name(),
        ms(injected_at)
    );

    match injected {
        Ok(()) => {
            if saved.is_some() || saved_primary.is_some() {
                host.restore_delay().await;
                for (selection, previous) in [
                    (Selection::Regular, saved),
                    (Selection::Primary, saved_primary),
                ] {
                    if let Some(previous) = previous {
                        // Do not overwrite something the user copied in the meantime.
                        if host.read(selection).await.ok().flatten().as_deref()
                            == Some(transcript.as_str())
                        {
                            if let Err(e) = host.write(previous, selection).await {
                                eprintln!("[clipboard] failed to restore clipboard: {e}");
                            }
                        }
                    }
                }
                eprintln!(
                    "[clipboard] pasteText t+{}ms: previous contents restored",
                    ms(started)
                );
            }
            Ok(SuccessMessageResult {
                success: true,
                message: Some("Paste keystroke sent".into()),
            })
        }
        Err(reason) => Ok(SuccessMessageResult {
            success: false,
            message: Some(format!(
                "Automatic paste failed: {reason} Your transcript is on the clipboard; use your application's Paste command."
            )),
        }),
    }
}

/// getSelectedTextViaCopy: inject Ctrl+C, poll the clipboard for a change,
/// restore the previous text contents. Only text can be saved/restored via
/// data-control here — a limitation over the macOS/Windows helpers which
/// restore all formats.
pub async fn get_selected_text_via_copy(
    clipboard: Clipboard,
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

    let before = match clipboard.get_text().await {
        Ok(text) => text,
        Err(e) => {
            return GetSelectedTextViaCopyResult {
                selected_text: None,
                clipboard_changed: false,
                message: Some(e),
            };
        }
    };

    if clipboard.focus_disturbed.swap(false, Ordering::Relaxed) {
        tokio::time::sleep(FOCUS_SETTLE_DELAY).await;
    }
    if let Err(e) = injector.inject_chord(MOD_CTRL, KEY_C).await {
        return GetSelectedTextViaCopyResult {
            selected_text: None,
            clipboard_changed: false,
            message: Some(format!("Failed to inject copy keystroke: {e}")),
        };
    }

    let mut after: Option<String> = None;
    let mut waited = Duration::ZERO;
    while waited < COPY_POLL_TOTAL {
        tokio::time::sleep(COPY_POLL_STEP).await;
        waited += COPY_POLL_STEP;
        match clipboard.get_text().await {
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
            if let Err(e) = clipboard.set_text(before).await {
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
