use super::*;
use std::sync::{Arc, Mutex};
#[path = "../tests/support/mod.rs"]
mod support;
use support::{status_changed, State, TestBus};

#[tokio::test]
async fn pauses_only_playing_controllable_players_and_resumes_once() {
    let bus = TestBus::new();
    let playing = Arc::new(Mutex::new(State::default()));
    let paused = Arc::new(Mutex::new(State {
        status: "Paused".into(),
        ..State::default()
    }));
    let unsupported = Arc::new(Mutex::new(State {
        can_pause: false,
        ..State::default()
    }));
    let rejected = Arc::new(Mutex::new(State {
        reject_pause: true,
        ..State::default()
    }));
    let _a = bus
        .player("org.mpris.MediaPlayer2.playing", playing.clone())
        .await;
    let _b = bus
        .player("org.mpris.MediaPlayer2.paused", paused.clone())
        .await;
    let _c = bus
        .player("org.mpris.MediaPlayer2.unsupported", unsupported.clone())
        .await;
    let _d = bus
        .player("org.mpris.MediaPlayer2.rejected", rejected.clone())
        .await;
    let mut media = MediaPlaybackService::default();
    media.pause_on(&bus.connection().await).await;
    assert_eq!(playing.lock().unwrap().status, "Paused");
    media.resume().await;
    media.resume().await;
    assert_eq!(playing.lock().unwrap().play_calls, 1);
    assert_eq!(paused.lock().unwrap().pause_calls, 0);
    assert_eq!(paused.lock().unwrap().play_calls, 0);
    assert_eq!(unsupported.lock().unwrap().pause_calls, 0);
    assert_eq!(rejected.lock().unwrap().play_calls, 0);
}

#[tokio::test]
async fn user_play_then_pause_invalidates_resume_even_when_final_state_is_paused() {
    let bus = TestBus::new();
    let state = Arc::new(Mutex::new(State::default()));
    let server = bus
        .player("org.mpris.MediaPlayer2.player", state.clone())
        .await;
    let mut media = MediaPlaybackService::default();
    media.pause_on(&bus.connection().await).await;
    status_changed(&server, &state, "Playing").await;
    status_changed(&server, &state, "Paused").await;
    media.resume().await;
    assert_eq!(state.lock().unwrap().play_calls, 0);
}

#[tokio::test]
async fn changed_track_or_disabled_play_controls_prevent_resume() {
    for change_track in [false, true] {
        let bus = TestBus::new();
        let state = Arc::new(Mutex::new(State::default()));
        let _server = bus
            .player("org.mpris.MediaPlayer2.player", state.clone())
            .await;
        let mut media = MediaPlaybackService::default();
        media.pause_on(&bus.connection().await).await;
        {
            let mut state = state.lock().unwrap();
            if change_track {
                state.track = "/track/two".into();
            } else {
                state.can_play = false;
            }
        }
        media.resume().await;
        assert_eq!(state.lock().unwrap().play_calls, 0);
    }
}

#[tokio::test]
async fn metadata_signal_invalidates_resume_even_if_original_track_returns() {
    let bus = TestBus::new();
    let state = Arc::new(Mutex::new(State::default()));
    let server = bus
        .player("org.mpris.MediaPlayer2.player", state.clone())
        .await;
    let mut media = MediaPlaybackService::default();
    media.pause_on(&bus.connection().await).await;
    let metadata = HashMap::from([(
        "mpris:trackid",
        zbus::zvariant::Value::from(zbus::zvariant::ObjectPath::try_from("/track/two").unwrap()),
    )]);
    let changed = HashMap::from([("Metadata", zbus::zvariant::Value::from(metadata))]);
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
    media.resume().await;
    assert_eq!(state.lock().unwrap().play_calls, 0);
}

#[tokio::test]
async fn closed_and_replaced_players_never_inherit_resume() {
    let bus = TestBus::new();
    let old = Arc::new(Mutex::new(State::default()));
    let server = bus
        .player("org.mpris.MediaPlayer2.player", old.clone())
        .await;
    let client = bus.connection().await;
    let mut media = MediaPlaybackService::default();
    media.pause_on(&client).await;
    server.close().await.unwrap();
    let replacement = Arc::new(Mutex::new(State {
        status: "Paused".into(),
        ..State::default()
    }));
    let _replacement = bus
        .player("org.mpris.MediaPlayer2.player", replacement.clone())
        .await;
    media.resume().await;
    assert_eq!(replacement.lock().unwrap().play_calls, 0);
}

#[tokio::test]
async fn aliases_of_the_same_player_are_paused_only_once() {
    let bus = TestBus::new();
    let state = Arc::new(Mutex::new(State::default()));
    let server = bus
        .player("org.mpris.MediaPlayer2.player", state.clone())
        .await;
    server
        .request_name("org.mpris.MediaPlayer2.alias")
        .await
        .unwrap();
    let mut media = MediaPlaybackService::default();
    media.pause_on(&bus.connection().await).await;
    media.resume().await;
    assert_eq!(state.lock().unwrap().pause_calls, 1);
    assert_eq!(state.lock().unwrap().play_calls, 1);
}

#[tokio::test]
async fn a_timed_out_pause_is_bounded_and_can_still_be_restored() {
    let bus = TestBus::new();
    let state = Arc::new(Mutex::new(State {
        slow_pause: true,
        ..State::default()
    }));
    let _server = bus
        .player("org.mpris.MediaPlayer2.player", state.clone())
        .await;
    let mut media = MediaPlaybackService::default();
    timeout(
        Duration::from_secs(2),
        media.pause_on(&bus.connection().await),
    )
    .await
    .unwrap();
    media.resume().await;
    assert_eq!(state.lock().unwrap().play_calls, 1);
}
