//! Release recovery for uinput builds when the portal misses Deactivated.
//! Read kernel key-state snapshots only while a registered chord is active;
//! never consume, grab, log, or forward arbitrary keyboard input.

use evdev::{Device, KeyCode};
use std::collections::HashSet;

use crate::keycodes::{modifier_for, Modifier};

pub struct ReleaseWatch {
    keyboards: Vec<Device>,
    required_modifiers: Vec<[KeyCode; 2]>,
    // Portal triggers are logical/layout-dependent. Capture the physical
    // non-modifier candidates at activation instead of assuming a US layout.
    trigger_candidates: Option<HashSet<KeyCode>>,
}

fn modifier_keys(modifier: Modifier) -> Option<[KeyCode; 2]> {
    Some(match modifier {
        Modifier::Ctrl => [KeyCode::KEY_LEFTCTRL, KeyCode::KEY_RIGHTCTRL],
        Modifier::Alt => [KeyCode::KEY_LEFTALT, KeyCode::KEY_RIGHTALT],
        Modifier::Shift => [KeyCode::KEY_LEFTSHIFT, KeyCode::KEY_RIGHTSHIFT],
        Modifier::Logo => [KeyCode::KEY_LEFTMETA, KeyCode::KEY_RIGHTMETA],
        Modifier::Fn => return None,
    })
}

fn is_modifier(key: KeyCode) -> bool {
    [
        Modifier::Ctrl,
        Modifier::Alt,
        Modifier::Shift,
        Modifier::Logo,
    ]
    .into_iter()
    .any(|modifier| modifier_keys(modifier).unwrap().contains(&key))
}

fn pressed_keys(keyboards: &[Device]) -> Option<HashSet<KeyCode>> {
    let mut pressed = HashSet::new();
    for keyboard in keyboards {
        // An unavailable device is not evidence of a released key.
        pressed.extend(keyboard.get_key_state().ok()?.iter());
    }
    Some(pressed)
}

fn released(
    required_modifiers: &[[KeyCode; 2]],
    trigger_candidates: Option<&HashSet<KeyCode>>,
    pressed: &HashSet<KeyCode>,
) -> bool {
    required_modifiers
        .iter()
        .any(|pair| !pair.iter().any(|key| pressed.contains(key)))
        || trigger_candidates.is_some_and(|keys| keys.is_disjoint(pressed))
}

impl ReleaseWatch {
    pub fn capture(chord: &[u32]) -> Option<Self> {
        let keyboards: Vec<_> = evdev::enumerate()
            .map(|(_, device)| device)
            .filter(|device| device.name() != Some("Amical Virtual Keyboard"))
            .filter(|device| {
                device
                    .supported_keys()
                    .is_some_and(|keys| keys.contains(KeyCode::KEY_A))
            })
            .collect();
        if keyboards.is_empty() {
            return None;
        }
        let mut required_modifiers = Vec::new();
        let mut has_trigger = false;
        for code in chord {
            match modifier_for(*code) {
                Some(modifier) => required_modifiers.push(modifier_keys(modifier)?),
                None => has_trigger = true,
            }
        }
        let pressed = pressed_keys(&keyboards)?;
        let trigger_candidates = has_trigger.then(|| {
            pressed
                .into_iter()
                .filter(|key| !is_modifier(*key))
                .collect()
        });
        Some(Self {
            keyboards,
            required_modifiers,
            trigger_candidates,
        })
    }

    pub fn is_released(&self) -> bool {
        pressed_keys(&self.keyboards).is_some_and(|pressed| {
            released(
                &self.required_modifiers,
                self.trigger_candidates.as_ref(),
                &pressed,
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_letter_or_modifier_release_without_a_portal_signal() {
        let modifiers = [
            modifier_keys(Modifier::Ctrl).unwrap(),
            modifier_keys(Modifier::Alt).unwrap(),
        ];
        // Deliberately a different physical key from Z: no layout assumption.
        let trigger = HashSet::from([KeyCode::KEY_W]);
        let held = HashSet::from([KeyCode::KEY_RIGHTCTRL, KeyCode::KEY_LEFTALT, KeyCode::KEY_W]);
        assert!(!released(&modifiers, Some(&trigger), &held));
        for key in &held {
            let mut remaining = held.clone();
            remaining.remove(key);
            assert!(released(&modifiers, Some(&trigger), &remaining));
        }
    }

    #[test]
    fn unrelated_key_release_does_not_cut_off_a_held_trigger() {
        let trigger = HashSet::from([KeyCode::KEY_W, KeyCode::KEY_SPACE]);
        assert!(!released(
            &[],
            Some(&trigger),
            &HashSet::from([KeyCode::KEY_W])
        ));
        assert!(released(&[], Some(&trigger), &HashSet::new()));
    }

    #[test]
    fn already_released_tap_and_modifier_only_chord() {
        assert!(released(&[], Some(&HashSet::new()), &HashSet::new()));
        let modifiers = [modifier_keys(Modifier::Shift).unwrap()];
        assert!(!released(
            &modifiers,
            None,
            &HashSet::from([KeyCode::KEY_RIGHTSHIFT])
        ));
        assert!(released(&modifiers, None, &HashSet::new()));
    }
}
