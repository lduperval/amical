//! Recording chrome: start/stop sounds, media pause/resume and system-audio muting.
//!
//! Audio *capture* happens in the Electron app itself (as on macOS/Windows —
//! note the start/stopRecording schemas carry no file path); this helper only
//! plays feedback sounds, pauses/resumes MPRIS players and mutes/restores output.
//!
//! Implemented by shelling out to `paplay`/`pactl`, which talk to PulseAudio
//! or PipeWire (via pipewire-pulse) alike — avoiding native audio library
//! dependencies in the helper binary. Missing tools degrade to no-ops with a
//! diagnostic on stderr.

use std::path::PathBuf;
use std::time::Duration;

use linux_helper::media_playback::MediaPlaybackService;
use tokio::process::Command;
use tokio::sync::Mutex;

use crate::rpc::{StartRecordingParams, StopRecordingParams, SuccessMessageResult};

const REC_START_WAV: &[u8] = include_bytes!("../resources/rec-start.wav");
const REC_STOP_WAV: &[u8] = include_bytes!("../resources/rec-stop.wav");

const DEFAULT_SINK: &str = "@DEFAULT_SINK@";
const SOUND_TIMEOUT: Duration = Duration::from_secs(5);

pub struct AudioService {
    /// Directory holding the unpacked sound files for paplay.
    sounds_dir: Option<tempfile::TempDir>,
    // Serialize recording transitions; rapid successor recordings can overlap
    // the previous session's cleanup in the desktop lifecycle.
    recording: Mutex<RecordingState>,
}

#[derive(Default)]
struct RecordingState {
    active: usize,
    previous_mute: Option<bool>,
    media: MediaPlaybackService,
}

impl AudioService {
    pub fn new() -> Self {
        let sounds_dir = match Self::unpack_sounds() {
            Ok(dir) => Some(dir),
            Err(e) => {
                eprintln!("[audio] failed to unpack sounds, playback disabled: {e}");
                None
            }
        };
        Self {
            sounds_dir,
            recording: Mutex::new(RecordingState::default()),
        }
    }

    fn unpack_sounds() -> std::io::Result<tempfile::TempDir> {
        let dir = tempfile::Builder::new()
            .prefix("amical-helper-")
            .tempdir()?;
        std::fs::write(dir.path().join("rec-start.wav"), REC_START_WAV)?;
        std::fs::write(dir.path().join("rec-stop.wav"), REC_STOP_WAV)?;
        Ok(dir)
    }

    fn sound_path(&self, name: &str) -> Option<PathBuf> {
        self.sounds_dir
            .as_ref()
            .map(|dir| dir.path().join(format!("{name}.wav")))
    }

    /// Play a feedback sound. `wait` matches the macOS helper's behavior of
    /// finishing the start sound before muting the system (so the sound
    /// itself isn't muted).
    async fn play_sound(&self, name: &str, wait: bool) {
        let Some(path) = self.sound_path(name) else {
            return;
        };
        let mut command = Command::new("paplay");
        command.arg(&path);
        match command.spawn() {
            Ok(mut child) => {
                if wait {
                    match tokio::time::timeout(SOUND_TIMEOUT, child.wait()).await {
                        Ok(Ok(status)) if !status.success() => {
                            eprintln!("[audio] paplay {name} exited with {status}");
                        }
                        Ok(Err(e)) => eprintln!("[audio] paplay {name} failed: {e}"),
                        Err(_) => {
                            eprintln!("[audio] paplay {name} timed out, killing");
                            let _ = child.kill().await;
                        }
                        _ => {}
                    }
                } else {
                    tokio::spawn(async move {
                        let _ = child.wait().await;
                    });
                }
            }
            Err(e) => eprintln!("[audio] could not run paplay: {e}"),
        }
    }

    async fn get_sink_mute(&self) -> Option<bool> {
        let output = tokio::time::timeout(
            Duration::from_millis(500),
            Command::new("pactl")
                .args(["get-sink-mute", DEFAULT_SINK])
                .env("LC_ALL", "C")
                .kill_on_drop(true)
                .output(),
        )
        .await
        .ok()?
        .ok()?;
        if !output.status.success() {
            return None;
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        Some(stdout.contains("yes"))
    }

    async fn set_sink_mute(&self, mute: bool) -> bool {
        match tokio::time::timeout(
            Duration::from_millis(500),
            Command::new("pactl")
                .args(["set-sink-mute", DEFAULT_SINK, if mute { "1" } else { "0" }])
                .kill_on_drop(true)
                .status(),
        )
        .await
        {
            Ok(Ok(status)) if status.success() => true,
            result => {
                eprintln!("[audio] could not update sink mute state: {result:?}");
                false
            }
        }
    }

    pub async fn start_recording(&self, params: StartRecordingParams) -> SuccessMessageResult {
        let mut state = self.recording.lock().await;
        let first = state.active == 0;
        state.active += 1;
        let mute_sounds = params.mute_sounds.unwrap_or(false);
        if !mute_sounds {
            self.play_sound("rec-start", true).await;
        }

        let mut success = true;
        if first && params.mute_system_audio {
            let previous = self.get_sink_mute().await;
            success = self.set_sink_mute(true).await;
            if success {
                state.previous_mute = Some(previous.unwrap_or(false));
            }
            // Media control remains independent of whether output muting worked.
            state.media.pause().await;
        }

        SuccessMessageResult {
            success,
            message: Some(if success {
                "Recording started".into()
            } else {
                "Failed to mute system audio".into()
            }),
        }
    }

    pub async fn stop_recording(&self, params: StopRecordingParams) -> SuccessMessageResult {
        let mut state = self.recording.lock().await;
        state.active = state.active.saturating_sub(1);
        let success = if state.active == 0 {
            self.restore(&mut state, params.was_muted).await
        } else {
            true
        };
        if !params.mute_sounds.unwrap_or(false) {
            self.play_sound("rec-stop", false).await;
        }
        SuccessMessageResult {
            success,
            message: Some(if success {
                "Recording stopped".into()
            } else {
                "Failed to restore system audio".into()
            }),
        }
    }

    async fn restore(&self, state: &mut RecordingState, was_muted: bool) -> bool {
        let previous = state.previous_mute.take().or(was_muted.then_some(false));
        let success = match previous {
            Some(previous) => self.set_sink_mute(previous).await,
            None => true,
        };
        // Also resume after cancellation, a mute failure, or a restore failure.
        state.media.resume().await;
        success
    }

    pub async fn shutdown(&self) {
        let mut state = self.recording.lock().await;
        state.active = 0;
        self.restore(&mut state, false).await;
    }
}
