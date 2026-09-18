//! Client for the Amical GNOME Shell extension (see `gnome-extension/`).
//!
//! The extension lives inside GNOME Shell and exposes, on the session bus,
//! the three things Mutter denies ordinary Wayland clients: key injection,
//! focus-free clipboard access, and placement of Amical's own windows. This
//! module is compiled into every helper variant so a `uinput` build can still
//! use the extension's clipboard (no focus stealing) when it is installed,
//! and so the desktop can manage the floating widget through it.
//!
//! Availability is probed lazily and cached briefly: the extension can be
//! enabled or disabled while the helper runs.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use tokio::sync::mpsc::UnboundedSender;
use zbus::{Connection, Proxy};

/// XKB real-modifier masks shared with the Wayland and uinput injectors.
pub const MOD_SHIFT: u32 = 1 << 0;
pub const MOD_CTRL: u32 = 1 << 2;
/// evdev key codes.
pub const KEY_C: u32 = 46;
pub const KEY_V: u32 = 47;
pub const KEY_INSERT: u32 = 110;

pub const DESTINATION: &str = "org.gnome.Shell";
pub const OBJECT_PATH: &str = "/org/gnome/Shell/Extensions/Amical";
pub const INTERFACE: &str = "org.gnome.Shell.Extensions.Amical";

/// Lowest extension API version this helper understands.
pub const MIN_API_VERSION: u32 = 1;

/// How long a positive or negative probe result stays valid.
const STATUS_CACHE_TTL: Duration = Duration::from_secs(5);
/// A chord is four key transitions with a 10 ms gap; clipboard calls are
/// immediate. Anything slower means the shell is wedged.
const CALL_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionStatus {
    pub version: u32,
    pub shell_version: String,
    pub keyboard: bool,
}

/// Which chord the extension should synthesize. The extension's allowlist is
/// the source of truth; these names must match its `CHORDS` table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chord {
    Paste,
    PastePrimary,
    Copy,
}

impl Chord {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Paste => "paste",
            Self::PastePrimary => "paste-primary",
            Self::Copy => "copy",
        }
    }

    /// Map the (XKB modifier mask, evdev key) pair used by the clipboard
    /// module onto an extension chord.
    pub fn from_mods_and_key(mods: u32, key: u32) -> Option<Self> {
        match (mods, key) {
            (MOD_CTRL, KEY_V) => Some(Self::Paste),
            (MOD_CTRL, KEY_C) => Some(Self::Copy),
            (MOD_SHIFT, KEY_INSERT) => Some(Self::PastePrimary),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WidgetPlacement<'a> {
    pub title: &'a str,
    pub x: i32,
    pub y: i32,
    pub above: bool,
    pub sticky: bool,
}

struct CachedStatus {
    checked: Instant,
    status: Option<ExtensionStatus>,
}

pub struct GnomeShell {
    conn: Connection,
    cache: Mutex<Option<CachedStatus>>,
}

impl GnomeShell {
    /// Connects to the session bus. Fails only when the bus itself is
    /// unreachable; extension absence surfaces from `status()`.
    pub async fn connect() -> Result<Self, String> {
        let conn = Connection::session()
            .await
            .map_err(|e| format!("session bus unavailable: {e}"))?;
        Ok(Self {
            conn,
            cache: Mutex::new(None),
        })
    }

    async fn proxy(&self) -> Result<Proxy<'static>, String> {
        Proxy::new(&self.conn, DESTINATION, OBJECT_PATH, INTERFACE)
            .await
            .map_err(|e| format!("extension proxy: {e}"))
    }

    /// Probe the extension. Cached for a few seconds in both directions so a
    /// paste does not add a second round trip and a missing extension does
    /// not spam the bus.
    pub async fn status(&self) -> Option<ExtensionStatus> {
        if let Some(cached) = self.cache.lock().ok().and_then(|c| {
            c.as_ref()
                .filter(|c| c.checked.elapsed() < STATUS_CACHE_TTL)
                .map(|c| c.status.clone())
        }) {
            return cached;
        }
        let status = self.fetch_status().await;
        if let Ok(mut cache) = self.cache.lock() {
            *cache = Some(CachedStatus {
                checked: Instant::now(),
                status: status.clone(),
            });
        }
        status
    }

    /// Drop the cached probe so the next call re-checks (used after a failed
    /// call: the extension may have been disabled).
    pub fn invalidate(&self) {
        if let Ok(mut cache) = self.cache.lock() {
            *cache = None;
        }
    }

    async fn fetch_status(&self) -> Option<ExtensionStatus> {
        let proxy = self.proxy().await.ok()?;
        let reply = tokio::time::timeout(CALL_TIMEOUT, proxy.call_method("GetStatus", &()))
            .await
            .ok()?
            .ok()?;
        let json: String = reply.body().deserialize().ok()?;
        let parsed: serde_json::Value = serde_json::from_str(&json).ok()?;
        let status = ExtensionStatus {
            version: parsed.get("version")?.as_u64()? as u32,
            shell_version: parsed
                .get("shellVersion")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned(),
            keyboard: parsed
                .get("keyboard")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
        };
        if status.version < MIN_API_VERSION {
            eprintln!(
                "[gnome-shell] extension API version {} is older than the supported {}",
                status.version, MIN_API_VERSION
            );
            return None;
        }
        Some(status)
    }

    /// True when the extension answers with a usable API version.
    pub async fn available(&self) -> bool {
        self.status().await.is_some()
    }

    pub async fn get_clipboard_text(&self, primary: bool) -> Result<Option<String>, String> {
        let proxy = self.proxy().await?;
        let reply = tokio::time::timeout(
            CALL_TIMEOUT,
            proxy.call_method("GetClipboardText", &(primary,)),
        )
        .await
        .map_err(|_| "extension clipboard read timed out".to_string())?
        .map_err(|e| self.failed("GetClipboardText", e))?;
        let (has_text, text): (bool, String) = reply
            .body()
            .deserialize()
            .map_err(|e| format!("extension clipboard reply: {e}"))?;
        Ok(has_text.then_some(text))
    }

    pub async fn set_clipboard_text(&self, text: &str, primary: bool) -> Result<(), String> {
        let proxy = self.proxy().await?;
        tokio::time::timeout(
            CALL_TIMEOUT,
            proxy.call_method("SetClipboardText", &(text, primary)),
        )
        .await
        .map_err(|_| "extension clipboard write timed out".to_string())?
        .map_err(|e| self.failed("SetClipboardText", e))?;
        Ok(())
    }

    /// Ask the shell to press and release a fixed chord on its virtual keyboard.
    pub async fn send_chord(&self, chord: Chord) -> Result<(), String> {
        let proxy = self.proxy().await?;
        tokio::time::timeout(
            CALL_TIMEOUT,
            proxy.call_method("SendChord", &(chord.name(),)),
        )
        .await
        .map_err(|_| "extension key injection timed out".to_string())?
        .map_err(|e| self.failed("SendChord", e))?;
        Ok(())
    }

    /// Returns whether the window was found now; the extension keeps the
    /// request and applies it to a later window with the same title too.
    pub async fn place_widget(&self, placement: WidgetPlacement<'_>) -> Result<bool, String> {
        let proxy = self.proxy().await?;
        let reply = tokio::time::timeout(
            CALL_TIMEOUT,
            proxy.call_method(
                "PlaceWidget",
                &(
                    placement.title,
                    placement.x,
                    placement.y,
                    placement.above,
                    placement.sticky,
                ),
            ),
        )
        .await
        .map_err(|_| "extension widget placement timed out".to_string())?
        .map_err(|e| self.failed("PlaceWidget", e))?;
        reply
            .body()
            .deserialize::<bool>()
            .map_err(|e| format!("extension placement reply: {e}"))
    }

    /// Focused window description as JSON (see the extension README), or
    /// None when nothing is focused or the extension is unavailable.
    pub async fn focused_window(&self) -> Option<serde_json::Value> {
        let proxy = self.proxy().await.ok()?;
        let reply = tokio::time::timeout(CALL_TIMEOUT, proxy.call_method("GetFocusedWindow", &()))
            .await
            .ok()?
            .ok()?;
        let json: String = reply.body().deserialize().ok()?;
        let value: serde_json::Value = serde_json::from_str(&json).ok()?;
        (!value.is_null()).then_some(value)
    }

    /// Forward the extension's `WidgetMoved` signals to the desktop as
    /// `widgetWindowMoved` helper events, for the lifetime of the helper.
    pub async fn forward_widget_moves(
        &self,
        out_tx: UnboundedSender<String>,
    ) -> Result<(), String> {
        let proxy = self.proxy().await?;
        let mut stream = proxy
            .receive_signal("WidgetMoved")
            .await
            .map_err(|e| format!("subscribe WidgetMoved: {e}"))?;
        tokio::spawn(async move {
            while let Some(msg) = stream.next().await {
                let Ok((title, x, y, width, height)) =
                    msg.body().deserialize::<(String, i32, i32, i32, i32)>()
                else {
                    continue;
                };
                let event = serde_json::json!({
                    "type": "widgetWindowMoved",
                    "payload": { "title": title, "x": x, "y": y, "width": width, "height": height },
                    "timestamp": time::OffsetDateTime::now_utc()
                        .format(&time::format_description::well_known::Rfc3339)
                        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string()),
                });
                let _ = out_tx.send(event.to_string());
            }
        });
        Ok(())
    }

    fn failed(&self, method: &str, error: zbus::Error) -> String {
        // The object vanishes when the extension is disabled; re-probe next time.
        self.invalidate();
        format!("GNOME Shell extension call {method} failed: {error}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chord_mapping_covers_exactly_the_injected_chords() {
        assert_eq!(Chord::from_mods_and_key(4, 47), Some(Chord::Paste));
        assert_eq!(Chord::from_mods_and_key(4, 46), Some(Chord::Copy));
        assert_eq!(Chord::from_mods_and_key(1, 110), Some(Chord::PastePrimary));
        assert_eq!(Chord::from_mods_and_key(1, 47), None);
        assert_eq!(Chord::from_mods_and_key(5, 110), None);
        assert_eq!(Chord::PastePrimary.name(), "paste-primary");
    }
}
