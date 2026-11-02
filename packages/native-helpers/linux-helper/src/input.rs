//! Select the actual input backend, not just the name printed at startup.

use std::sync::Arc;

use linux_helper::gnome_shell::GnomeShell;
use linux_helper::{current_method, InputMethod};

#[derive(Clone)]
pub enum KeyInjector {
    Wayland(crate::wayland::KeyInjector),
    #[cfg(feature = "uinput")]
    Uinput(Arc<linux_helper::uinput::UInputInjector>),
    #[cfg(feature = "gnome_ext")]
    GnomeExt(Arc<linux_helper::gnome_ext::GnomeExtInjector>),
}

impl KeyInjector {
    #[allow(unused_variables)]
    pub fn new(shell: Arc<GnomeShell>) -> Result<Self, String> {
        match current_method() {
            InputMethod::Clipboard => crate::wayland::KeyInjector::new()
                .map(Self::Wayland)
                .ok_or_else(|| "Wayland virtual keyboard unavailable".into()),
            #[cfg(feature = "uinput")]
            InputMethod::Uinput => linux_helper::uinput::UInputInjector::new()
                .map(|device| Self::Uinput(Arc::new(device))),
            #[cfg(feature = "gnome_ext")]
            InputMethod::GnomeExt => Ok(Self::GnomeExt(Arc::new(
                linux_helper::gnome_ext::GnomeExtInjector::new(shell),
            ))),
            method => Err(format!(
                "input method '{}' is not implemented in this helper build",
                method.as_str()
            )),
        }
    }

    pub fn backend_name(&self) -> &'static str {
        match self {
            Self::Wayland(_) => "wayland-virtual-keyboard",
            #[cfg(feature = "uinput")]
            Self::Uinput(_) => "uinput",
            #[cfg(feature = "gnome_ext")]
            Self::GnomeExt(_) => "gnome-extension",
        }
    }

    /// Whether the backend can deliver a chord right now.
    pub async fn available(&self) -> bool {
        match self {
            Self::Wayland(_) => true,
            #[cfg(feature = "uinput")]
            Self::Uinput(_) => true,
            #[cfg(feature = "gnome_ext")]
            Self::GnomeExt(injector) => injector.available().await,
        }
    }

    /// Shift+Insert with both selections supplied (uinput, extension) versus
    /// Ctrl+V with the regular clipboard only (Wayland virtual keyboard).
    pub fn uses_primary_paste(&self) -> bool {
        match self {
            Self::Wayland(_) => false,
            #[cfg(feature = "uinput")]
            Self::Uinput(_) => true,
            #[cfg(feature = "gnome_ext")]
            Self::GnomeExt(_) => true,
        }
    }

    /// Press and release `mods`+`key`. Errors carry a user-facing reason.
    pub async fn inject_chord(&self, mods: u32, key: u32) -> Result<(), String> {
        match self {
            Self::Wayland(device) => {
                let device = device.clone();
                let ok =
                    tokio::task::spawn_blocking(move || device.inject_chord_blocking(mods, key))
                        .await
                        .unwrap_or(false);
                ok.then_some(())
                    .ok_or_else(|| "the compositor rejected the virtual keyboard events".into())
            }
            #[cfg(feature = "uinput")]
            Self::Uinput(device) => {
                let device = device.clone();
                tokio::task::spawn_blocking(move || device.inject_chord(mods, key))
                    .await
                    .unwrap_or_else(|e| Err(format!("injection task panicked: {e}")))
            }
            #[cfg(feature = "gnome_ext")]
            Self::GnomeExt(injector) => injector.inject_chord(mods, key).await,
        }
    }
}
