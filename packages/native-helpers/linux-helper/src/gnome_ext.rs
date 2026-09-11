//! GNOME Shell Extension key injection integration (Option C).
//!
//! Interacts with a companion GNOME Shell extension via DBus or a Unix domain
//! socket to inject keystrokes or directly insert text into the focused window.
//!
//! Under GNOME Wayland, GNOME Mutter restricts `zwp_virtual_keyboard_v1` from
//! unprivileged client applications. A GNOME Shell extension runs inside Mutter's
//! process space and has full privileges to emit Clutter key events or insert
//! text directly.

#[cfg(feature = "gnome_ext")]
#[derive(Debug, Default)]
pub struct GnomeExtInjector {
    extension_id: String,
}

#[cfg(feature = "gnome_ext")]
impl GnomeExtInjector {
    /// Connect to the GNOME Shell extension IPC endpoint.
    pub fn new() -> Result<Self, String> {
        // Will check for extension availability via DBus
        Ok(Self {
            extension_id: "amical@amical.ai".into(),
        })
    }

    /// Request the GNOME Shell extension inject a key chord or paste text.
    pub fn inject_chord(&self, mods: u32, key: u32) -> Result<(), String> {
        eprintln!(
            "[gnome_ext] inject_chord called (mods: {mods:#x}, key: {key}) via extension {}",
            self.extension_id
        );
        // DBus call to org.gnome.Shell.Extensions.Amical
        Ok(())
    }
}
