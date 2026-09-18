//! GNOME Shell extension key injection (Option C).
//!
//! Under GNOME Wayland, Mutter refuses `zwp_virtual_keyboard_v1` to ordinary
//! clients. The Amical GNOME Shell extension (see `gnome-extension/`) runs
//! inside the shell and presses a fixed set of copy/paste chords on a Clutter
//! virtual keyboard when asked over the session bus. This injector is the
//! helper-side half of that pair; it needs no `/dev/uinput` access.
//!
//! The injector is constructed even when the extension is not (yet) running so
//! the helper keeps answering; every injection re-checks availability and
//! fails with an actionable message instead.

use std::sync::Arc;

use crate::gnome_shell::{Chord, GnomeShell};

pub struct GnomeExtInjector {
    shell: Arc<GnomeShell>,
}

impl GnomeExtInjector {
    pub fn new(shell: Arc<GnomeShell>) -> Self {
        Self { shell }
    }

    pub async fn available(&self) -> bool {
        self.shell
            .status()
            .await
            .is_some_and(|status| status.keyboard)
    }

    /// Press and release a chord through the extension. Only the chords in
    /// the extension's allowlist can be expressed; anything else is refused
    /// here before touching the bus.
    pub async fn inject_chord(&self, mods: u32, key: u32) -> Result<(), String> {
        let chord = Chord::from_mods_and_key(mods, key).ok_or_else(|| {
            format!("unsupported chord for the GNOME extension: mods={mods:#x} key={key}")
        })?;
        match self.shell.status().await {
            None => return Err(crate::gnome_ext_missing_message().into()),
            Some(status) if !status.keyboard => {
                return Err("The Amical GNOME Shell extension could not create its virtual keyboard; check `journalctl --user -f` for GNOME Shell errors.".into())
            }
            Some(_) => {}
        }
        self.shell.send_chord(chord).await
    }
}
