//! Recording chrome: start/stop sounds and system-audio muting.
//!
//! Audio *capture* happens in the Electron app itself (as on macOS/Windows —
//! note the start/stopRecording schemas carry no file path); this helper only
//! plays the feedback sounds and mutes/restores the system output sink.
//!
//! Implemented by shelling out to `paplay`/`pactl`, which talk to PulseAudio
//! or PipeWire (via pipewire-pulse) alike — avoiding native audio library
//! dependencies in the helper binary. Missing tools degrade to no-ops with a
//! diagnostic on stderr.

use std::path::PathBuf;
use std::time::Duration;

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
    /// Sink mute state before we muted it, captured by startRecording so
    /// stopRecording can put it back instead of blindly unmuting.
    previous_mute: Mutex<Option<bool>>,
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
            previous_mute: Mutex::new(None),
        }
    }

    fn unpack_sounds() -> std::io::Result<tempfile::TempDir> {
        let dir = tempfile::Builder::new().prefix("amical-helper-").tempdir()?;
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
        let output = Command::new("pactl")
            .args(["get-sink-mute", DEFAULT_SINK])
            .output()
            .await
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        Some(stdout.contains("yes"))
    }

    async fn set_sink_mute(&self, mute: bool) -> bool {
        match Command::new("pactl")
            .args(["set-sink-mute", DEFAULT_SINK, if mute { "1" } else { "0" }])
            .status()
            .await
        {
            Ok(status) if status.success() => true,
            Ok(status) => {
                eprintln!("[audio] pactl set-sink-mute exited with {status}");
                false
            }
            Err(e) => {
                eprintln!("[audio] could not run pactl: {e}");
                false
            }
        }
    }

    pub async fn start_recording(&self, params: StartRecordingParams) -> SuccessMessageResult {
        let mute_sounds = params.mute_sounds.unwrap_or(false);
        if !mute_sounds {
            self.play_sound("rec-start", true).await;
        }

        let mut success = true;
        if params.mute_system_audio {
            let previous = self.get_sink_mute().await;
            success = self.set_sink_mute(true).await;
            if success {
                *self.previous_mute.lock().await = previous;
            }
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
        let mute_sounds = params.mute_sounds.unwrap_or(false);

        let mut success = true;
        if params.was_muted {
            // Restore the pre-recording state; default to unmuting when the
            // helper restarted in between and lost it.
            let previous = self.previous_mute.lock().await.take().unwrap_or(false);
            success = self.set_sink_mute(previous).await;
        }

        if !mute_sounds {
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
}
