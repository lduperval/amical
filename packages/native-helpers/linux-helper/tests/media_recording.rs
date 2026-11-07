//! Exercise the shipped JSON-RPC lifecycle, with fake audio tools and private MPRIS.
mod support;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use support::{State, TestBus};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::time::timeout;

struct Helper {
    child: Child,
    input: Option<ChildStdin>,
    output: Lines<BufReader<ChildStdout>>,
    tools: tempfile::TempDir,
    next_id: usize,
}
impl Helper {
    fn new(bus: &TestBus) -> Self {
        let tools = tempfile::tempdir().unwrap();
        let pactl = tools.path().join("pactl");
        std::fs::write(&pactl, r##"#!/bin/sh
if [ "$1" = "get-sink-mute" ]; then
  if [ "$(cat "$AMICAL_TEST_TOOLS/mute" 2>/dev/null)" = "1" ]; then echo 'Mute: yes'; else echo 'Mute: no'; fi
elif [ "$1" = "set-sink-mute" ]; then
  if [ "$3" = "1" ] && [ -f "$AMICAL_TEST_TOOLS/fail-mute" ]; then exit 1; fi
  if [ "$3" = "0" ] && [ -f "$AMICAL_TEST_TOOLS/fail-restore" ]; then exit 1; fi
  echo "$3" > "$AMICAL_TEST_TOOLS/mute"
fi
"##).unwrap();
        std::fs::set_permissions(&pactl, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_linux-helper"))
            .env("DBUS_SESSION_BUS_ADDRESS", &bus.address)
            .env("AMICAL_INPUT_METHOD", "clipboard")
            .env("PATH", format!("{}:/usr/bin:/bin", tools.path().display()))
            .env("AMICAL_TEST_TOOLS", tools.path())
            .env("XDG_RUNTIME_DIR", tools.path())
            .env("WAYLAND_DISPLAY", "no-test-compositor")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        Self {
            input: child.stdin.take(),
            output: BufReader::new(child.stdout.take().unwrap()).lines(),
            child,
            tools,
            next_id: 0,
        }
    }
    fn file(&self, name: &str) -> PathBuf {
        self.tools.path().join(name)
    }
    async fn call(&mut self, method: &str, params: serde_json::Value) -> bool {
        self.next_id += 1;
        let id = self.next_id.to_string();
        let request =
            serde_json::json!({"jsonrpc":"2.0", "id":id, "method":method, "params":params});
        let input = self.input.as_mut().unwrap();
        input
            .write_all(format!("{request}\n").as_bytes())
            .await
            .unwrap();
        input.flush().await.unwrap();
        timeout(Duration::from_secs(5), async {
            loop {
                let line = self
                    .output
                    .next_line()
                    .await
                    .unwrap()
                    .expect("helper exited before response");
                let response: serde_json::Value = serde_json::from_str(&line).unwrap();
                if response["id"] == id {
                    return response["result"]["success"].as_bool().unwrap();
                }
            }
        })
        .await
        .unwrap()
    }
    async fn start(&mut self, enabled: bool) -> bool {
        self.call(
            "startRecording",
            serde_json::json!({"muteSystemAudio":enabled, "muteSounds":true}),
        )
        .await
    }
    async fn stop(&mut self, was_muted: bool) -> bool {
        self.call(
            "stopRecording",
            serde_json::json!({"wasMuted":was_muted, "muteSounds":true}),
        )
        .await
    }
}
impl Drop for Helper {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

#[tokio::test]
async fn test_bus_cannot_activate_desktop_accessibility_or_portal_services() {
    let bus = TestBus::new();
    let connection = bus.connection().await;
    let proxy = zbus::fdo::DBusProxy::new(&connection).await.unwrap();
    let activatable = proxy.list_activatable_names().await.unwrap();
    for service in [
        "org.a11y.Bus",
        "org.freedesktop.portal.Desktop",
        "org.freedesktop.secrets",
    ] {
        assert!(!activatable.iter().any(|name| name.as_str() == service));
        assert!(proxy
            .start_service_by_name(service.try_into().unwrap(), 0)
            .await
            .is_err());
        assert!(!proxy
            .name_has_owner(service.try_into().unwrap())
            .await
            .unwrap());
    }
}

#[tokio::test]
async fn recording_pauses_and_stop_or_cancellation_resumes_media() {
    let bus = TestBus::new();
    let state = Arc::new(Mutex::new(State {
        seeked_on_pause: true,
        ..State::default()
    }));
    let _player = bus
        .player("org.mpris.MediaPlayer2.player", state.clone())
        .await;
    let mut helper = Helper::new(&bus);
    for _ in 0..2 {
        assert!(helper.start(true).await);
        assert_eq!(state.lock().unwrap().status, "Paused");
        assert!(helper.stop(true).await);
        assert_eq!(state.lock().unwrap().status, "Playing");
    }
    assert_eq!(state.lock().unwrap().pause_calls, 2);
    assert_eq!(state.lock().unwrap().play_calls, 2);
    assert_eq!(
        std::fs::read_to_string(helper.file("mute")).unwrap().trim(),
        "0"
    );
}

#[tokio::test]
async fn disabled_preference_does_not_touch_media_or_system_audio() {
    let bus = TestBus::new();
    let state = Arc::new(Mutex::new(State::default()));
    let _player = bus
        .player("org.mpris.MediaPlayer2.player", state.clone())
        .await;
    let mut helper = Helper::new(&bus);
    assert!(helper.start(false).await);
    assert!(helper.stop(false).await);
    assert_eq!(state.lock().unwrap().pause_calls, 0);
    assert!(!helper.file("mute").exists());
}

#[tokio::test]
async fn overlapping_dictations_resume_only_after_the_last_stop_and_preserve_initial_mute() {
    let bus = TestBus::new();
    let state = Arc::new(Mutex::new(State::default()));
    let _player = bus
        .player("org.mpris.MediaPlayer2.player", state.clone())
        .await;
    let mut helper = Helper::new(&bus);
    std::fs::write(helper.file("mute"), "1").unwrap();
    assert!(helper.start(true).await);
    assert!(helper.start(true).await);
    assert!(helper.stop(true).await);
    assert_eq!(state.lock().unwrap().status, "Paused");
    assert!(helper.stop(true).await);
    assert_eq!(state.lock().unwrap().play_calls, 1);
    assert_eq!(state.lock().unwrap().pause_calls, 1);
    assert_eq!(
        std::fs::read_to_string(helper.file("mute")).unwrap().trim(),
        "1"
    );
}

#[tokio::test]
async fn failed_muting_or_restore_cannot_leave_media_paused() {
    for fail_start in [false, true] {
        let bus = TestBus::new();
        let state = Arc::new(Mutex::new(State::default()));
        let _player = bus
            .player("org.mpris.MediaPlayer2.player", state.clone())
            .await;
        let mut helper = Helper::new(&bus);
        if fail_start {
            std::fs::write(helper.file("fail-mute"), "").unwrap();
        }
        assert_eq!(helper.start(true).await, !fail_start);
        assert_eq!(state.lock().unwrap().status, "Paused");
        if !fail_start {
            std::fs::write(helper.file("fail-restore"), "").unwrap();
        }
        assert_eq!(helper.stop(!fail_start).await, fail_start);
        assert_eq!(state.lock().unwrap().status, "Playing");
    }
}

#[tokio::test]
async fn parent_disconnect_restores_playback_without_a_stop_rpc() {
    let bus = TestBus::new();
    let state = Arc::new(Mutex::new(State::default()));
    let _player = bus
        .player("org.mpris.MediaPlayer2.player", state.clone())
        .await;
    let mut helper = Helper::new(&bus);
    assert!(helper.start(true).await);
    helper.input.take();
    timeout(Duration::from_secs(2), async {
        loop {
            if state.lock().unwrap().play_calls == 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(helper.file("mute")).unwrap().trim(),
        "0"
    );
}
