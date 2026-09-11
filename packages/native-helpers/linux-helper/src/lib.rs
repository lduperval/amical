//! Linux native helper library for Amical.
//!
//! Provides configurable input-method implementations:
//! - Default / `Clipboard`: Standard Wayland clipboard access (user presses Ctrl+V if compositor blocks injection)
//! - `uinput`: Direct `/dev/uinput` kernel key injection (bypasses compositor restrictions)
//! - `xtest`: X11 / XTest extension key injection
//! - `gnome_ext`: GNOME Shell Extension integration

// Compile-time checks: ensure at most one optional input method feature is selected
#[cfg(all(feature = "uinput", feature = "xtest"))]
compile_error!(
    "Features 'uinput' and 'xtest' are mutually exclusive. Please enable only one input method feature."
);

#[cfg(all(feature = "uinput", feature = "gnome_ext"))]
compile_error!(
    "Features 'uinput' and 'gnome_ext' are mutually exclusive. Please enable only one input method feature."
);

#[cfg(all(feature = "xtest", feature = "gnome_ext"))]
compile_error!(
    "Features 'xtest' and 'gnome_ext' are mutually exclusive. Please enable only one input method feature."
);

#[cfg(feature = "uinput")]
pub mod uinput;

#[cfg(feature = "xtest")]
pub mod xtest;

#[cfg(feature = "gnome_ext")]
pub mod gnome_ext;

/// Represents the active or compiled input method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMethod {
    /// Default clipboard-based approach (Ctrl+V fallback if key injection is unavailable).
    Clipboard,
    /// Linux `/dev/uinput` kernel virtual device injection.
    Uinput,
    /// X11 XTest extension key injection.
    XTest,
    /// GNOME Shell Extension IPC injection.
    GnomeExt,
}

impl InputMethod {
    /// Parse an input method string identifier.
    pub fn from_str_name(s: &str) -> Option<Self> {
        match s {
            "clipboard" => Some(Self::Clipboard),
            "uinput" => Some(Self::Uinput),
            "xtest" => Some(Self::XTest),
            "gnome_ext" => Some(Self::GnomeExt),
            _ => None,
        }
    }

    /// String representation matching CLI and configuration flags.
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Clipboard => "clipboard",
            Self::Uinput => "uinput",
            Self::XTest => "xtest",
            Self::GnomeExt => "gnome_ext",
        }
    }
}

/// Returns the compile-time configured input method based on active Cargo features.
pub fn compile_time_method() -> InputMethod {
    #[cfg(feature = "uinput")]
    {
        return InputMethod::Uinput;
    }
    #[cfg(feature = "xtest")]
    {
        return InputMethod::XTest;
    }
    #[cfg(feature = "gnome_ext")]
    {
        return InputMethod::GnomeExt;
    }
    #[cfg(not(any(feature = "uinput", feature = "xtest", feature = "gnome_ext")))]
    {
        InputMethod::Clipboard
    }
}

/// Returns the currently active input method.
/// Checks the `AMICAL_INPUT_METHOD` environment variable first, falling back to
/// the compile-time default feature selection.
pub fn current_method() -> InputMethod {
    if let Ok(val) = std::env::var("AMICAL_INPUT_METHOD") {
        if let Some(method) = InputMethod::from_str_name(val.trim()) {
            return method;
        }
    }
    compile_time_method()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compile_time_method_matches_active_feature() {
        let method = compile_time_method();
        #[cfg(feature = "uinput")]
        assert_eq!(method, InputMethod::Uinput);
        #[cfg(feature = "xtest")]
        assert_eq!(method, InputMethod::XTest);
        #[cfg(feature = "gnome_ext")]
        assert_eq!(method, InputMethod::GnomeExt);
        #[cfg(not(any(feature = "uinput", feature = "xtest", feature = "gnome_ext")))]
        assert_eq!(method, InputMethod::Clipboard);
    }

    #[test]
    fn test_input_method_string_conversion() {
        assert_eq!(InputMethod::Clipboard.as_str(), "clipboard");
        assert_eq!(InputMethod::Uinput.as_str(), "uinput");
        assert_eq!(InputMethod::XTest.as_str(), "xtest");
        assert_eq!(InputMethod::GnomeExt.as_str(), "gnome_ext");

        assert_eq!(
            InputMethod::from_str_name("clipboard"),
            Some(InputMethod::Clipboard)
        );
        assert_eq!(
            InputMethod::from_str_name("uinput"),
            Some(InputMethod::Uinput)
        );
        assert_eq!(
            InputMethod::from_str_name("xtest"),
            Some(InputMethod::XTest)
        );
        assert_eq!(
            InputMethod::from_str_name("gnome_ext"),
            Some(InputMethod::GnomeExt)
        );
        assert_eq!(InputMethod::from_str_name("invalid"), None);
    }
}
