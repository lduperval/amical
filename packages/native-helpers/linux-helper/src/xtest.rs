//! X11 / XTest key injection fallback (Option B).
//!
//! Synthesizes keystrokes for standard X11 sessions (or XWayland clients) using
//! the X11 XTest extension (`XTestFakeKeyEvent`).
//!
//! This method is suitable when running Amical in an X11 desktop environment
//! (XFCE, LXQt, or GNOME/KDE under X11) where Wayland protocols are not used.

#[cfg(feature = "xtest")]
#[derive(Debug, Default)]
pub struct XTestInjector {
    display_name: Option<String>,
}

#[cfg(feature = "xtest")]
impl XTestInjector {
    /// Initialize connection to the X11 display.
    pub fn new() -> Result<Self, String> {
        let display = std::env::var("DISPLAY").map_err(|_| {
            "DISPLAY environment variable not set. X11 session required.".to_string()
        })?;

        Ok(Self {
            display_name: Some(display),
        })
    }

    /// Inject a key chord (e.g. Ctrl+V or Ctrl+C) using XTestFakeKeyEvent.
    pub fn inject_chord(&self, mods: u32, key: u32) -> Result<(), String> {
        eprintln!(
            "[xtest] inject_chord called (mods: {mods:#x}, key: {key}) on display {:?}",
            self.display_name
        );
        // XTest implementation will call XTestFakeKeyEvent via x11rb / libX11 / XTest
        Ok(())
    }
}
