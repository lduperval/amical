//! Global shortcuts via the `org.freedesktop.portal.GlobalShortcuts` portal.
//!
//! Wayland prohibits global input sniffing, so unlike the macOS/Windows
//! helpers we never see raw key events. Instead each chord from `setShortcuts`
//! is registered as a portal shortcut; when the portal reports
//! Activated/Deactivated we synthesize the `keyDown`/`keyUp` events the
//! desktop expects, echoing back the exact keycodes it registered. That keeps
//! the desktop's chord matching (ShortcutManager) working unchanged — the
//! keycode space is whatever the desktop uses, we never interpret it beyond
//! building human-readable labels.
//!
//! Consequences of the portal model, documented for posterity:
//! - Key events only fire for registered chords, never for arbitrary keys.
//! - Rebinding requires closing the session and creating a new one; some
//!   desktops (GNOME) show a confirmation dialog the first time.
//! - "Consuming" keys is the compositor's job here: an activated global
//!   shortcut is not delivered to the focused app, which is exactly the
//!   swallow behavior the other helpers implement by hand.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use futures_util::StreamExt;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::Mutex;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};
use zbus::{Connection, Proxy};

use crate::keycodes::{self, Modifier};
use crate::rpc::{HelperEvent, KeyEventPayload, SetShortcutsParams};

const PORTAL_DEST: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const GLOBAL_SHORTCUTS_IFACE: &str = "org.freedesktop.portal.GlobalShortcuts";
const REQUEST_IFACE: &str = "org.freedesktop.portal.Request";
const SESSION_IFACE: &str = "org.freedesktop.portal.Session";

/// Portal Response(u, a{sv}) success code.
const RESPONSE_SUCCESS: u32 = 0;

/// How long to wait for a portal Response before giving up. BindShortcuts can
/// pop an interactive approval dialog, so its completion is handled in the
/// background rather than awaited here (the desktop's RPC timeout is 5s).
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(25);

struct SessionState {
    /// Currently bound chords: portal shortcut id -> desktop keycodes.
    chords: HashMap<String, Vec<u32>>,
    /// Portal session the chords are bound in.
    session: Option<OwnedObjectPath>,
    /// Shortcut ids the portal currently reports as held down.
    active: HashSet<String>,
}

pub struct ShortcutsService {
    conn: Connection,
    sender_token: String,
    state: Arc<Mutex<SessionState>>,
    event_tx: UnboundedSender<String>,
    token_counter: AtomicU64,
}

impl ShortcutsService {
    /// Connects to the session bus and starts the Activated/Deactivated
    /// listeners. Fails (returning Err) only when DBus itself is unreachable;
    /// portal absence surfaces later as setShortcuts failures.
    pub async fn new(event_tx: UnboundedSender<String>) -> Result<Arc<Self>, String> {
        let conn = Connection::session()
            .await
            .map_err(|e| format!("session bus unavailable: {e}"))?;

        let sender_token = conn
            .unique_name()
            .ok_or("no unique bus name")?
            .trim_start_matches(':')
            .replace('.', "_");

        let service = Arc::new(Self {
            conn,
            sender_token,
            state: Arc::new(Mutex::new(SessionState {
                chords: HashMap::new(),
                session: None,
                active: HashSet::new(),
            })),
            event_tx,
            token_counter: AtomicU64::new(0),
        });

        service.clone().spawn_signal_listener("Activated").await?;
        service.clone().spawn_signal_listener("Deactivated").await?;

        Ok(service)
    }

    async fn portal_proxy(&self) -> Result<Proxy<'static>, String> {
        Proxy::new(
            &self.conn,
            PORTAL_DEST,
            PORTAL_PATH,
            GLOBAL_SHORTCUTS_IFACE,
        )
        .await
        .map_err(|e| format!("portal proxy: {e}"))
    }

    async fn spawn_signal_listener(self: Arc<Self>, signal: &'static str) -> Result<(), String> {
        let proxy = self.portal_proxy().await?;
        let mut stream = proxy
            .receive_signal(signal)
            .await
            .map_err(|e| format!("subscribe {signal}: {e}"))?;

        tokio::spawn(async move {
            while let Some(msg) = stream.next().await {
                let parsed: Result<(OwnedObjectPath, String, u64, HashMap<String, OwnedValue>), _> =
                    msg.body().deserialize();
                let Ok((session, shortcut_id, _timestamp, _options)) = parsed else {
                    continue;
                };
                self.handle_signal(signal, session, shortcut_id).await;
            }
            eprintln!("[shortcuts] {signal} signal stream ended");
        });

        Ok(())
    }

    async fn handle_signal(&self, signal: &str, session: OwnedObjectPath, shortcut_id: String) {
        let mut state = self.state.lock().await;
        if state.session.as_ref() != Some(&session) {
            return;
        }
        let Some(keycodes) = state.chords.get(&shortcut_id).cloned() else {
            return;
        };

        let (event_type, activated) = match signal {
            "Activated" => ("keyDown", true),
            _ => ("keyUp", false),
        };
        if activated {
            state.active.insert(shortcut_id.clone());
        } else {
            state.active.remove(&shortcut_id);
        }
        drop(state);

        eprintln!("[shortcuts] {signal}: {shortcut_id}");
        self.emit_chord_events(event_type, &keycodes);
    }

    /// Synthesize one key event per chord keycode. Modifier booleans reflect
    /// the modifiers contained in the chord (the desktop only consumes
    /// `keyCode`, the rest is informational).
    fn emit_chord_events(&self, event_type: &'static str, keycodes_list: &[u32]) {
        let mut alt = false;
        let mut ctrl = false;
        let mut shift = false;
        let mut meta = false;
        for &code in keycodes_list {
            match keycodes::modifier_for(code) {
                Some(Modifier::Alt) => alt = true,
                Some(Modifier::Ctrl) => ctrl = true,
                Some(Modifier::Shift) => shift = true,
                Some(Modifier::Logo) => meta = true,
                _ => {}
            }
        }

        // keyDown in chord order, keyUp in reverse (mirrors how a user
        // releases a chord and keeps the desktop's active-key set coherent).
        let ordered: Vec<u32> = if event_type == "keyUp" {
            keycodes_list.iter().rev().copied().collect()
        } else {
            keycodes_list.to_vec()
        };

        for code in ordered {
            let event = HelperEvent {
                event_type,
                payload: KeyEventPayload {
                    key: Some(keycodes::display_name(code)),
                    code: None,
                    key_code: code,
                    alt_key: alt,
                    ctrl_key: ctrl,
                    shift_key: shift,
                    meta_key: meta,
                },
                timestamp: crate::rpc::iso_timestamp(),
            };
            if let Ok(line) = serde_json::to_string(&event) {
                let _ = self.event_tx.send(line);
            }
        }
    }

    fn next_token(&self) -> String {
        format!(
            "amical_{}",
            self.token_counter.fetch_add(1, Ordering::Relaxed)
        )
    }

    /// Subscribe to the Response signal of the request object the portal will
    /// create for `token`, BEFORE issuing the call — the response can arrive
    /// immediately.
    async fn request_response_stream(
        &self,
        token: &str,
    ) -> Result<(zbus::proxy::SignalStream<'static>, String), String> {
        let request_path = format!(
            "/org/freedesktop/portal/desktop/request/{}/{}",
            self.sender_token, token
        );
        let proxy: Proxy<'static> = Proxy::new(
            &self.conn,
            PORTAL_DEST,
            request_path.clone(),
            REQUEST_IFACE,
        )
        .await
        .map_err(|e| format!("request proxy: {e}"))?;
        let stream = proxy
            .receive_signal("Response")
            .await
            .map_err(|e| format!("subscribe Response: {e}"))?;
        Ok((stream, request_path))
    }

    async fn await_response(
        mut stream: zbus::proxy::SignalStream<'static>,
        what: &str,
    ) -> Result<(u32, HashMap<String, OwnedValue>), String> {
        let msg = tokio::time::timeout(REQUEST_TIMEOUT, stream.next())
            .await
            .map_err(|_| format!("{what}: portal response timed out"))?
            .ok_or_else(|| format!("{what}: portal response stream closed"))?;
        msg.body()
            .deserialize::<(u32, HashMap<String, OwnedValue>)>()
            .map_err(|e| format!("{what}: bad response body: {e}"))
    }

    async fn close_session(&self, session: &OwnedObjectPath) {
        match Proxy::new(&self.conn, PORTAL_DEST, session.clone(), SESSION_IFACE).await {
            Ok(proxy) => {
                if let Err(e) = proxy.call_method("Close", &()).await {
                    eprintln!("[shortcuts] closing old session failed: {e}");
                }
            }
            Err(e) => eprintln!("[shortcuts] session proxy failed: {e}"),
        }
    }

    /// Register the desktop's chords with the portal. Creates a fresh session
    /// each time (BindShortcuts is once-per-session by portal design).
    pub async fn set_shortcuts(self: &Arc<Self>, params: SetShortcutsParams) -> Result<(), String> {
        // The native side treats both groups identically (see the schema TODO
        // in set-shortcuts.ts); dedupe merged chords.
        let mut chords: Vec<Vec<u32>> = Vec::new();
        for chord in params
            .subset_chords
            .into_iter()
            .chain(params.exact_chords)
        {
            if !chord.is_empty() && !chords.contains(&chord) {
                chords.push(chord);
            }
        }

        // Tear down any previous session; its bindings die with it.
        let old_session = {
            let mut state = self.state.lock().await;
            state.active.clear();
            state.session.take()
        };
        if let Some(session) = old_session {
            self.close_session(&session).await;
        }

        if chords.is_empty() {
            let mut state = self.state.lock().await;
            state.chords.clear();
            return Ok(());
        }

        let proxy = self.portal_proxy().await?;

        // --- CreateSession ---------------------------------------------------
        let handle_token = self.next_token();
        let session_token = self.next_token();
        let (response_stream, _) = self.request_response_stream(&handle_token).await?;

        let mut options: HashMap<&str, Value> = HashMap::new();
        options.insert("handle_token", Value::from(handle_token.as_str()));
        options.insert("session_handle_token", Value::from(session_token.as_str()));
        proxy
            .call_method("CreateSession", &(options))
            .await
            .map_err(|e| format!("CreateSession: {e}"))?;

        let (code, results) = Self::await_response(response_stream, "CreateSession").await?;
        if code != RESPONSE_SUCCESS {
            return Err(format!("CreateSession denied (response code {code})"));
        }
        let session_handle = results
            .get("session_handle")
            .and_then(|v| match &**v {
                // The spec says 's'; be lenient and accept 'o' too.
                Value::Str(s) => Some(s.to_string()),
                Value::ObjectPath(p) => Some(p.to_string()),
                _ => None,
            })
            .ok_or("CreateSession: no session_handle in results")?;
        let session = OwnedObjectPath::try_from(session_handle.as_str())
            .map_err(|e| format!("CreateSession: bad session handle: {e}"))?;

        // --- BindShortcuts ---------------------------------------------------
        let bind_token = self.next_token();
        let (bind_stream, _) = self.request_response_stream(&bind_token).await?;

        let mut shortcut_entries: Vec<(String, HashMap<&str, Value>)> = Vec::new();
        let mut chord_map: HashMap<String, Vec<u32>> = HashMap::new();
        for chord in &chords {
            let id = format!(
                "chord-{}",
                chord
                    .iter()
                    .map(|c| c.to_string())
                    .collect::<Vec<_>>()
                    .join("-")
            );
            let mut entry: HashMap<&str, Value> = HashMap::new();
            entry.insert("description", Value::from(chord_description(chord)));
            if let Some(trigger) = chord_trigger(chord) {
                entry.insert("preferred_trigger", Value::from(trigger));
            }
            shortcut_entries.push((id.clone(), entry));
            chord_map.insert(id, chord.clone());
        }

        let mut bind_options: HashMap<&str, Value> = HashMap::new();
        bind_options.insert("handle_token", Value::from(bind_token.as_str()));
        proxy
            .call_method(
                "BindShortcuts",
                &(session.clone(), shortcut_entries, "", bind_options),
            )
            .await
            .map_err(|e| format!("BindShortcuts: {e}"))?;

        // Install the new state now: activation signals may start before the
        // Response (and on GNOME the Response waits on a user dialog).
        {
            let mut state = self.state.lock().await;
            state.chords = chord_map;
            state.session = Some(session);
        }

        // Await the bind result in the background so an approval dialog can't
        // wedge the RPC; failure just logs (the desktop already treats
        // setShortcuts as best-effort).
        let service = self.clone();
        tokio::spawn(async move {
            match Self::await_response(bind_stream, "BindShortcuts").await {
                Ok((RESPONSE_SUCCESS, _)) => {
                    eprintln!("[shortcuts] shortcuts bound");
                }
                Ok((code, _)) => {
                    eprintln!("[shortcuts] BindShortcuts denied (response code {code})");
                    let mut state = service.state.lock().await;
                    state.chords.clear();
                    state.active.clear();
                }
                Err(e) => eprintln!("[shortcuts] {e}"),
            }
        });

        Ok(())
    }

    /// The portal tells us exactly which chords are held (Activated without a
    /// matching Deactivated), so stale keys are the pressed keys that don't
    /// belong to any currently-active chord.
    pub async fn stale_keys(&self, pressed: &[u32]) -> Vec<u32> {
        let state = self.state.lock().await;
        let mut held: HashSet<u32> = HashSet::new();
        for id in &state.active {
            if let Some(chord) = state.chords.get(id) {
                held.extend(chord.iter().copied());
            }
        }
        pressed
            .iter()
            .copied()
            .filter(|code| !held.contains(code))
            .collect()
    }
}

/// Human-readable label for the compositor's shortcut settings UI.
fn chord_description(chord: &[u32]) -> String {
    let keys = chord
        .iter()
        .map(|&c| keycodes::display_name(c))
        .collect::<Vec<_>>()
        .join("+");
    format!("Amical {keys}")
}

/// XDG shortcuts-spec trigger string (e.g. "CTRL+ALT+space"), or None when
/// the chord has no expressible trigger (Fn-based chords, multiple
/// non-modifier keys, modifier-only chords).
fn chord_trigger(chord: &[u32]) -> Option<String> {
    let mut mods: Vec<&'static str> = Vec::new();
    let mut key: Option<String> = None;

    for &code in chord {
        match keycodes::modifier_for(code) {
            Some(Modifier::Ctrl) => mods.push("CTRL"),
            Some(Modifier::Alt) => mods.push("ALT"),
            Some(Modifier::Shift) => mods.push("SHIFT"),
            Some(Modifier::Logo) => mods.push("LOGO"),
            Some(Modifier::Fn) => return None,
            None => {
                if key.is_some() {
                    return None;
                }
                key = Some(keycodes::xkb_keysym_name(code)?);
            }
        }
    }

    let key = key?;
    mods.dedup();
    let mut trigger = mods.join("+");
    if !trigger.is_empty() {
        trigger.push('+');
    }
    trigger.push_str(&key);
    Some(trigger)
}
