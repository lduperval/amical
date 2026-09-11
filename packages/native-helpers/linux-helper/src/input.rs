//! Select the actual input backend, not just the name printed at startup.

use linux_helper::{current_method, InputMethod};

#[derive(Clone)]
pub enum KeyInjector {
    Wayland(crate::wayland::KeyInjector),
    #[cfg(feature = "uinput")]
    Uinput(std::sync::Arc<linux_helper::uinput::UInputInjector>),
}

impl KeyInjector {
    pub fn new() -> Result<Self, String> {
        match current_method() {
            InputMethod::Clipboard => crate::wayland::KeyInjector::new()
                .map(Self::Wayland)
                .ok_or_else(|| "Wayland virtual keyboard unavailable".into()),
            #[cfg(feature = "uinput")]
            InputMethod::Uinput => linux_helper::uinput::UInputInjector::new()
                .map(|device| Self::Uinput(std::sync::Arc::new(device))),
            method => Err(format!(
                "input method '{}' is not implemented in this helper build",
                method.as_str()
            )),
        }
    }

    pub fn uses_primary_paste(&self) -> bool {
        match self {
            Self::Wayland(_) => false,
            #[cfg(feature = "uinput")]
            Self::Uinput(_) => true,
        }
    }

    pub fn inject_chord_blocking(&self, mods: u32, key: u32) -> bool {
        match self {
            Self::Wayland(device) => device.inject_chord_blocking(mods, key),
            #[cfg(feature = "uinput")]
            Self::Uinput(device) => match device.inject_chord(mods, key) {
                Ok(()) => true,
                Err(e) => {
                    eprintln!("[uinput] key injection failed: {e}");
                    false
                }
            },
        }
    }
}
