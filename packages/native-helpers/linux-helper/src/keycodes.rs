//! Keycode tables for the desktop's keycode space.
//!
//! On Linux the desktop app reuses the macOS keycode table
//! (`apps/desktop/src/utils/keycode-map.ts` falls back to the macOS map on
//! non-Windows platforms), so shortcut chords arrive here as macOS virtual
//! keycodes and the key events we synthesize must echo those same codes back.
//! This module only translates them to human-readable names (for portal
//! shortcut descriptions) and to XKB keysym names (for portal
//! `preferred_trigger` strings, per the freedesktop shortcuts spec).

/// XDG shortcuts-spec modifier for a macOS modifier keycode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modifier {
    Ctrl,
    Alt,
    Shift,
    Logo,
    /// The macOS Fn key: a modifier for chord bookkeeping, but with no XDG
    /// trigger equivalent.
    Fn,
}

pub fn modifier_for(keycode: u32) -> Option<Modifier> {
    match keycode {
        55 | 54 => Some(Modifier::Logo),  // Cmd / RCmd
        59 | 62 => Some(Modifier::Ctrl),  // Ctrl / RCtrl
        58 | 61 => Some(Modifier::Alt),   // Alt / RAlt
        56 | 60 => Some(Modifier::Shift), // Shift / RShift
        63 => Some(Modifier::Fn),
        _ => None,
    }
}

/// Human-readable name, mirroring keycode-map.ts (used in portal shortcut
/// descriptions shown by the compositor's settings UI).
pub fn display_name(keycode: u32) -> String {
    match keycode {
        0 => "A".into(),
        1 => "S".into(),
        2 => "D".into(),
        3 => "F".into(),
        4 => "H".into(),
        5 => "G".into(),
        6 => "Z".into(),
        7 => "X".into(),
        8 => "C".into(),
        9 => "V".into(),
        11 => "B".into(),
        12 => "Q".into(),
        13 => "W".into(),
        14 => "E".into(),
        15 => "R".into(),
        16 => "Y".into(),
        17 => "T".into(),
        31 => "O".into(),
        32 => "U".into(),
        34 => "I".into(),
        35 => "P".into(),
        37 => "L".into(),
        38 => "J".into(),
        40 => "K".into(),
        45 => "N".into(),
        46 => "M".into(),
        18 => "1".into(),
        19 => "2".into(),
        20 => "3".into(),
        21 => "4".into(),
        22 => "6".into(),
        23 => "5".into(),
        25 => "9".into(),
        26 => "7".into(),
        28 => "8".into(),
        29 => "0".into(),
        48 => "Tab".into(),
        49 => "Space".into(),
        51 => "Delete".into(),
        36 | 52 => "Enter".into(),
        53 => "Escape".into(),
        57 => "CapsLock".into(),
        117 => "ForwardDelete".into(),
        55 => "Cmd".into(),
        54 => "RCmd".into(),
        59 => "Ctrl".into(),
        62 => "RCtrl".into(),
        58 => "Alt".into(),
        61 => "RAlt".into(),
        56 => "Shift".into(),
        60 => "RShift".into(),
        63 => "Fn".into(),
        122 => "F1".into(),
        120 => "F2".into(),
        99 => "F3".into(),
        118 => "F4".into(),
        96 => "F5".into(),
        97 => "F6".into(),
        98 => "F7".into(),
        100 => "F8".into(),
        101 => "F9".into(),
        109 => "F10".into(),
        103 => "F11".into(),
        111 => "F12".into(),
        105 => "F13".into(),
        107 => "F14".into(),
        113 => "F15".into(),
        106 => "F16".into(),
        64 => "F17".into(),
        79 => "F18".into(),
        80 => "F19".into(),
        90 => "F20".into(),
        115 => "Home".into(),
        116 => "PageUp".into(),
        121 => "PageDown".into(),
        119 => "End".into(),
        114 => "Help".into(),
        123 => "Left".into(),
        124 => "Right".into(),
        125 => "Down".into(),
        126 => "Up".into(),
        27 => "-".into(),
        24 => "=".into(),
        33 => "[".into(),
        30 => "]".into(),
        42 => "\\".into(),
        41 => ";".into(),
        39 => "'".into(),
        43 => ",".into(),
        47 => ".".into(),
        44 => "/".into(),
        50 => "`".into(),
        76 => "KeypadEnter".into(),
        other => format!("Key{other}"),
    }
}

/// XKB keysym name for the non-modifier part of a portal trigger string, or
/// None when the key has no reasonable keysym mapping.
pub fn xkb_keysym_name(keycode: u32) -> Option<String> {
    let name = match keycode {
        // Letters: keysym is the lowercase letter.
        0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 11 | 12 | 13 | 14 | 15 | 16 | 17 | 31 | 32 | 34
        | 35 | 37 | 38 | 40 | 45 | 46 => display_name(keycode).to_lowercase(),
        // Digits.
        18 | 19 | 20 | 21 | 22 | 23 | 25 | 26 | 28 | 29 => display_name(keycode),
        48 => "Tab".into(),
        49 => "space".into(),
        51 => "BackSpace".into(),
        36 | 52 => "Return".into(),
        53 => "Escape".into(),
        57 => "Caps_Lock".into(),
        117 => "Delete".into(),
        122 | 120 | 99 | 118 | 96 | 97 | 98 | 100 | 101 | 109 | 103 | 111 | 105 | 107 | 113
        | 106 | 64 | 79 | 80 | 90 => display_name(keycode),
        115 => "Home".into(),
        116 => "Page_Up".into(),
        121 => "Page_Down".into(),
        119 => "End".into(),
        123 => "Left".into(),
        124 => "Right".into(),
        125 => "Down".into(),
        126 => "Up".into(),
        27 => "minus".into(),
        24 => "equal".into(),
        33 => "bracketleft".into(),
        30 => "bracketright".into(),
        42 => "backslash".into(),
        41 => "semicolon".into(),
        39 => "apostrophe".into(),
        43 => "comma".into(),
        47 => "period".into(),
        44 => "slash".into(),
        50 => "grave".into(),
        76 => "KP_Enter".into(),
        _ => return None,
    };
    Some(name)
}
