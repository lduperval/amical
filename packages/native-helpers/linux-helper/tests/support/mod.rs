//! Shared fake player used by protocol and helper-process tests.
#![allow(dead_code)]
//! Real MPRIS protocol tests on private buses; never touch the user's players.
use std::collections::HashMap;
use std::time::Duration;
use zbus::zvariant::OwnedValue;
use zbus::Connection;
const PATH: &str = "/org/mpris/MediaPlayer2";
const PLAYER: &str = "org.mpris.MediaPlayer2.Player";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
type Properties = HashMap<String, OwnedValue>;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use zbus::{connection, SignalContext};

pub struct TestBus {
    daemon: Child,
    pub address: String,
}
impl TestBus {
    pub fn new() -> Self {
        let mut daemon = Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--print-address=1"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut address = String::new();
        BufReader::new(daemon.stdout.take().unwrap())
            .read_line(&mut address)
            .unwrap();
        Self {
            daemon,
            address: address.trim().to_owned(),
        }
    }
    pub async fn connection(&self) -> Connection {
        connection::Builder::address(self.address.as_str())
            .unwrap()
            .build()
            .await
            .unwrap()
    }
    pub async fn player(&self, name: &str, state: Arc<Mutex<State>>) -> Connection {
        connection::Builder::address(self.address.as_str())
            .unwrap()
            .name(name)
            .unwrap()
            .serve_at(PATH, FakePlayer(state))
            .unwrap()
            .build()
            .await
            .unwrap()
    }
}
impl Drop for TestBus {
    fn drop(&mut self) {
        let _ = self.daemon.kill();
        let _ = self.daemon.wait();
    }
}

pub struct State {
    pub status: String,
    pub track: String,
    pub can_pause: bool,
    pub can_play: bool,
    pub reject_pause: bool,
    pub slow_pause: bool,
    pub pause_calls: usize,
    pub play_calls: usize,
}
impl Default for State {
    fn default() -> Self {
        Self {
            status: "Playing".into(),
            track: "/track/one".into(),
            can_pause: true,
            can_play: true,
            reject_pause: false,
            slow_pause: false,
            pause_calls: 0,
            play_calls: 0,
        }
    }
}

struct FakePlayer(Arc<Mutex<State>>);
#[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
impl FakePlayer {
    async fn pause(
        &self,
        #[zbus(signal_context)] context: SignalContext<'_>,
    ) -> zbus::fdo::Result<()> {
        let slow = {
            let mut state = self.0.lock().unwrap();
            state.pause_calls += 1;
            if state.reject_pause {
                return Err(zbus::fdo::Error::Failed("player refused".into()));
            }
            state.status = "Paused".into();
            state.slow_pause
        };
        self.playback_status_changed(&context).await?;
        if slow {
            tokio::time::sleep(Duration::from_millis(650)).await;
        }
        Ok(())
    }
    async fn play(
        &self,
        #[zbus(signal_context)] context: SignalContext<'_>,
    ) -> zbus::fdo::Result<()> {
        {
            let mut state = self.0.lock().unwrap();
            state.play_calls += 1;
            state.status = "Playing".into();
        }
        self.playback_status_changed(&context).await?;
        Ok(())
    }
    #[zbus(property)]
    fn playback_status(&self) -> String {
        self.0.lock().unwrap().status.clone()
    }
    #[zbus(property)]
    fn can_control(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_pause(&self) -> bool {
        self.0.lock().unwrap().can_pause
    }
    #[zbus(property)]
    fn can_play(&self) -> bool {
        self.0.lock().unwrap().can_play
    }
    #[zbus(property)]
    fn metadata(&self) -> Properties {
        let track = self.0.lock().unwrap().track.clone();
        HashMap::from([(
            "mpris:trackid".to_owned(),
            OwnedValue::from(zbus::zvariant::ObjectPath::try_from(track).unwrap()),
        )])
    }
}

pub async fn status_changed(server: &Connection, state: &Arc<Mutex<State>>, status: &str) {
    state.lock().unwrap().status = status.into();
    let changed = HashMap::from([("PlaybackStatus", zbus::zvariant::Value::from(status))]);
    server
        .emit_signal(
            None::<&str>,
            PATH,
            PROPERTIES,
            "PropertiesChanged",
            &(PLAYER, changed, Vec::<&str>::new()),
        )
        .await
        .unwrap();
}
