//! Wayland connection and key injection via `zwp_virtual_keyboard_v1`.
//!
//! Wayland prohibits clients from injecting input into other clients through
//! core protocols. The virtual-keyboard protocol is the one portable-ish
//! escape hatch: wlroots compositors (Sway, Hyprland, river, ...) and KWin
//! expose it to regular clients; GNOME's Mutter does not. When the global is
//! absent this module reports itself unavailable and callers degrade
//! gracefully (clipboard-only paste, no copy-capture).
//!
//! The injector runs on a dedicated thread owning the Wayland event queue;
//! async callers talk to it over a channel.

use std::io::Write;
use std::os::fd::AsFd;
use std::sync::mpsc as std_mpsc;
use std::time::{Duration, Instant};

use wayland_client::protocol::{wl_registry, wl_seat};
use wayland_client::{delegate_noop, Connection, Dispatch, QueueHandle};
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1;
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1;

/// Minimal self-contained XKB keymap covering exactly the keys we inject.
/// Keycodes are evdev + 8: LCTL=37 (KEY_LEFTCTRL 29), LFSH=50 (KEY_LEFTSHIFT
/// 42), KC=54 (KEY_C 46), KV=55 (KEY_V 47), INS=118 (KEY_INSERT 110).
const KEYMAP: &str = include_str!("keymap.xkb");

// evdev key codes (what zwp_virtual_keyboard_v1::key expects).
pub const KEY_C: u32 = 46;
pub const KEY_V: u32 = 47;

// Real modifier masks are positional and fixed in XKB (Shift=1<<0, Ctrl=1<<2).
pub const MOD_CTRL: u32 = 1 << 2;

const KEY_STATE_PRESSED: u32 = 1;
const KEY_STATE_RELEASED: u32 = 0;
const KEYMAP_FORMAT_XKB_V1: u32 = 1;

/// Delay between synthetic key transitions. Compositors forward virtual
/// keyboard events immediately, but the focused client processes them on its
/// own loop; a small gap keeps chords from being coalesced or reordered.
const STEP_DELAY: Duration = Duration::from_millis(12);

struct InjectCommand {
    mods: u32,
    key: u32,
    reply: std_mpsc::Sender<bool>,
}

#[derive(Default)]
struct RegistryState {
    seat: Option<wl_seat::WlSeat>,
    manager: Option<ZwpVirtualKeyboardManagerV1>,
}

impl Dispatch<wl_registry::WlRegistry, ()> for RegistryState {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            match interface.as_str() {
                "wl_seat" if state.seat.is_none() => {
                    state.seat =
                        Some(registry.bind::<wl_seat::WlSeat, _, _>(name, version.min(5), qh, ()));
                }
                "zwp_virtual_keyboard_manager_v1" => {
                    state.manager = Some(registry.bind::<ZwpVirtualKeyboardManagerV1, _, _>(
                        name,
                        1,
                        qh,
                        (),
                    ));
                }
                _ => {}
            }
        }
    }
}

delegate_noop!(RegistryState: ignore wl_seat::WlSeat);
delegate_noop!(RegistryState: ignore ZwpVirtualKeyboardManagerV1);
delegate_noop!(RegistryState: ignore ZwpVirtualKeyboardV1);

/// Handle to the injector thread. Cheap to clone.
#[derive(Clone)]
pub struct KeyInjector {
    tx: std_mpsc::Sender<InjectCommand>,
}

impl KeyInjector {
    /// Connects to the compositor and sets up a virtual keyboard.
    /// Returns None when the compositor doesn't offer the protocol (e.g.
    /// GNOME) or no Wayland display is reachable.
    pub fn new() -> Option<Self> {
        let (setup_tx, setup_rx) = std_mpsc::channel::<Result<(), String>>();
        let (cmd_tx, cmd_rx) = std_mpsc::channel::<InjectCommand>();

        std::thread::Builder::new()
            .name("wayland-injector".into())
            .spawn(move || injector_thread(setup_tx, cmd_rx))
            .ok()?;

        match setup_rx.recv() {
            Ok(Ok(())) => {
                eprintln!("[wayland] virtual keyboard ready");
                Some(Self { tx: cmd_tx })
            }
            Ok(Err(reason)) => {
                eprintln!("[wayland] key injection unavailable: {reason}");
                None
            }
            Err(_) => {
                eprintln!("[wayland] injector thread died during setup");
                None
            }
        }
    }

    /// Press `mods` + `key` (evdev code) and release, e.g. Ctrl+V.
    /// Blocking; call from spawn_blocking in async context.
    pub fn inject_chord_blocking(&self, mods: u32, key: u32) -> bool {
        let (reply_tx, reply_rx) = std_mpsc::channel();
        if self
            .tx
            .send(InjectCommand {
                mods,
                key,
                reply: reply_tx,
            })
            .is_err()
        {
            return false;
        }
        reply_rx
            .recv_timeout(Duration::from_secs(2))
            .unwrap_or(false)
    }
}

fn injector_thread(
    setup_tx: std_mpsc::Sender<Result<(), String>>,
    cmd_rx: std_mpsc::Receiver<InjectCommand>,
) {
    let setup = || -> Result<(Connection, wayland_client::EventQueue<RegistryState>, RegistryState, ZwpVirtualKeyboardV1), String> {
        let conn = Connection::connect_to_env()
            .map_err(|e| format!("no Wayland display: {e}"))?;
        let mut queue = conn.new_event_queue();
        let qh = queue.handle();
        let display = conn.display();
        display.get_registry(&qh, ());

        let mut state = RegistryState::default();
        queue
            .roundtrip(&mut state)
            .map_err(|e| format!("registry roundtrip failed: {e}"))?;

        let seat = state
            .seat
            .clone()
            .ok_or_else(|| "compositor exposes no wl_seat".to_string())?;
        let manager = state
            .manager
            .clone()
            .ok_or_else(|| "compositor does not support zwp_virtual_keyboard_manager_v1".to_string())?;

        let keyboard = manager.create_virtual_keyboard(&seat, &qh, ());

        // Upload the keymap through a sealed temp file (fd + size, NUL included).
        let mut file = tempfile::tempfile().map_err(|e| format!("keymap tempfile: {e}"))?;
        file.write_all(KEYMAP.as_bytes())
            .and_then(|_| file.write_all(&[0]))
            .map_err(|e| format!("keymap write: {e}"))?;
        keyboard.keymap(
            KEYMAP_FORMAT_XKB_V1,
            file.as_fd(),
            (KEYMAP.len() + 1) as u32,
        );

        queue
            .roundtrip(&mut state)
            .map_err(|e| format!("keymap roundtrip failed: {e}"))?;

        Ok((conn, queue, state, keyboard))
    };

    let (_conn, mut queue, mut state, keyboard) = match setup() {
        Ok(parts) => {
            let _ = setup_tx.send(Ok(()));
            parts
        }
        Err(wayland_err) => {
            eprintln!("[wayland] zwp_virtual_keyboard_v1 unavailable ({wayland_err}), falling back to Mutter RemoteDesktop");
            match setup_mutter() {
                Ok(mutter_state) => {
                    let _ = setup_tx.send(Ok(()));
                    eprintln!("[wayland] Mutter RemoteDesktop key injection ready");
                    run_mutter_loop(mutter_state, cmd_rx);
                    return;
                }
                Err(mutter_err) => {
                    let _ = setup_tx.send(Err(format!(
                        "wayland error: {wayland_err}; mutter error: {mutter_err}"
                    )));
                    return;
                }
            }
        }
    };

    let started = Instant::now();
    let now_ms = |started: Instant| started.elapsed().as_millis() as u32;

    while let Ok(cmd) = cmd_rx.recv() {
        let ok = (|| -> Result<(), wayland_client::backend::WaylandError> {
            keyboard.modifiers(cmd.mods, 0, 0, 0);
            queue.flush()?;
            std::thread::sleep(STEP_DELAY);

            keyboard.key(now_ms(started), cmd.key, KEY_STATE_PRESSED);
            queue.flush()?;
            std::thread::sleep(STEP_DELAY);

            keyboard.key(now_ms(started), cmd.key, KEY_STATE_RELEASED);
            keyboard.modifiers(0, 0, 0, 0);
            queue.flush()?;
            Ok(())
        })()
        .is_ok();

        // Drain any compositor events and confirm delivery.
        let ok = ok && queue.roundtrip(&mut state).is_ok();
        let _ = cmd.reply.send(ok);
    }
}

struct MutterState {
    session_proxy: zbus::blocking::Proxy<'static>,
}

fn setup_mutter() -> Result<MutterState, String> {
    let conn = zbus::blocking::Connection::session()
        .map_err(|e| format!("session bus unavailable: {e}"))?;
    let proxy = zbus::blocking::Proxy::new(
        &conn,
        "org.gnome.Mutter.RemoteDesktop",
        "/org/gnome/Mutter/RemoteDesktop",
        "org.gnome.Mutter.RemoteDesktop",
    )
    .map_err(|e| format!("Mutter RemoteDesktop proxy failed: {e}"))?;

    let session_path: zbus::zvariant::OwnedObjectPath = proxy
        .call_method("CreateSession", &())
        .map_err(|e| format!("CreateSession failed: {e}"))?
        .body()
        .deserialize()
        .map_err(|e| format!("Failed to parse session path: {e}"))?;

    let session_proxy = zbus::blocking::Proxy::new(
        &conn,
        "org.gnome.Mutter.RemoteDesktop",
        session_path,
        "org.gnome.Mutter.RemoteDesktop.Session",
    )
    .map_err(|e| format!("Session proxy failed: {e}"))?;

    session_proxy
        .call_method("Start", &())
        .map_err(|e| format!("Session Start failed: {e}"))?;

    Ok(MutterState { session_proxy })
}

const XKB_KEY_CONTROL_L: u32 = 0xffe3;
const XKB_KEY_SHIFT_L: u32 = 0xffe1;
const XKB_KEY_V: u32 = 0x0076;
const XKB_KEY_C: u32 = 0x0063;
const XKB_KEY_INSERT: u32 = 0xff63;

fn run_mutter_loop(
    state: MutterState,
    cmd_rx: std_mpsc::Receiver<InjectCommand>,
) {
    while let Ok(cmd) = cmd_rx.recv() {
        let ok = inject_mutter_chord(&state.session_proxy, cmd.mods, cmd.key);
        let _ = cmd.reply.send(ok);
    }
}

fn inject_mutter_chord(
    proxy: &zbus::blocking::Proxy<'static>,
    mods: u32,
    key: u32,
) -> bool {
    let keysym = match key {
        KEY_V => XKB_KEY_V,
        KEY_C => XKB_KEY_C,
        110 => XKB_KEY_INSERT,
        _ => return false,
    };

    let ctrl = (mods & MOD_CTRL) != 0;
    let shift = (mods & (1 << 0)) != 0;

    if ctrl {
        if proxy
            .call_method("NotifyKeyboardKeysym", &(XKB_KEY_CONTROL_L, true))
            .is_err()
        {
            return false;
        }
        std::thread::sleep(STEP_DELAY);
    }

    if shift {
        if proxy
            .call_method("NotifyKeyboardKeysym", &(XKB_KEY_SHIFT_L, true))
            .is_err()
        {
            if ctrl {
                let _ = proxy.call_method("NotifyKeyboardKeysym", &(XKB_KEY_CONTROL_L, false));
            }
            return false;
        }
        std::thread::sleep(STEP_DELAY);
    }

    if proxy
        .call_method("NotifyKeyboardKeysym", &(keysym, true))
        .is_err()
    {
        if shift {
            let _ = proxy.call_method("NotifyKeyboardKeysym", &(XKB_KEY_SHIFT_L, false));
        }
        if ctrl {
            let _ = proxy.call_method("NotifyKeyboardKeysym", &(XKB_KEY_CONTROL_L, false));
        }
        return false;
    }
    std::thread::sleep(STEP_DELAY);

    let _ = proxy.call_method("NotifyKeyboardKeysym", &(keysym, false));
    std::thread::sleep(STEP_DELAY);

    if shift {
        let _ = proxy.call_method("NotifyKeyboardKeysym", &(XKB_KEY_SHIFT_L, false));
        std::thread::sleep(STEP_DELAY);
    }

    if ctrl {
        let _ = proxy.call_method("NotifyKeyboardKeysym", &(XKB_KEY_CONTROL_L, false));
    }

    true
}
