//! A persistent kernel virtual keyboard. Requires write access to /dev/uinput
//! and read access to physical keyboards to wait for held modifiers to clear.

use std::io;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use evdev::{uinput::VirtualDevice, AttributeSet, Device, EventType, InputEvent, KeyCode};

const DEVICE_NAME: &str = "Amical Virtual Keyboard";
const STEP_DELAY: Duration = Duration::from_millis(12);
const RELEASE_TIMEOUT: Duration = Duration::from_secs(2);
const RELEASE_POLL: Duration = Duration::from_millis(25);

const MODIFIERS: [KeyCode; 8] = [
    KeyCode::KEY_LEFTSHIFT,
    KeyCode::KEY_RIGHTSHIFT,
    KeyCode::KEY_LEFTCTRL,
    KeyCode::KEY_RIGHTCTRL,
    KeyCode::KEY_LEFTALT,
    KeyCode::KEY_RIGHTALT,
    KeyCode::KEY_LEFTMETA,
    KeyCode::KEY_RIGHTMETA,
];

#[derive(Debug)]
pub struct UInputInjector {
    device: Mutex<VirtualDevice>,
}

impl UInputInjector {
    pub fn new() -> Result<Self, String> {
        let mut keys = AttributeSet::<KeyCode>::new();
        // Advertise a keyboard (udev's keyboard classification needs the letter
        // range), not just a two-button input device. We only emit paste/copy.
        for code in 1..=127 {
            keys.insert(KeyCode(code));
        }
        let device = VirtualDevice::builder()
            .and_then(|builder| builder.name(DEVICE_NAME).with_keys(&keys))
            .and_then(|builder| builder.build())
            .map_err(|e| format!("cannot create virtual keyboard via /dev/uinput: {e}; check the uinput module and device permissions, then restart Amical"))?;

        // A newly created device must be discovered by udev/libinput before it
        // can deliver events. Keep it alive for the entire helper lifetime.
        std::thread::sleep(Duration::from_secs(1));
        Ok(Self {
            device: Mutex::new(device),
        })
    }

    pub fn inject_chord(&self, mods: u32, key: u32) -> Result<(), String> {
        let modifier_keys = chord_modifiers(mods)?;
        let key = match key {
            46 => KeyCode::KEY_C,
            47 => KeyCode::KEY_V,
            110 => KeyCode::KEY_INSERT,
            _ => return Err(format!("unsupported injected key: {key}")),
        };
        let mut device = self
            .device
            .lock()
            .map_err(|_| "virtual keyboard lock poisoned")?;

        // The portal may deactivate the shortcut when just its letter is
        // released. Check *all* physical modifiers, independent of the user's
        // configured shortcut; never synthesize releases on physical devices.
        let keyboards: Vec<Device> = evdev::enumerate()
            .map(|(_, device)| device)
            .filter(|device| device.name() != Some(DEVICE_NAME))
            .filter(|device| {
                device
                    .supported_keys()
                    .is_some_and(|keys| MODIFIERS.iter().any(|key| keys.contains(*key)))
            })
            .collect();
        if keyboards.is_empty() {
            return Err("cannot read keyboard modifier state; check read access to /dev/input/event* and restart Amical after joining the input group".into());
        }
        wait_for_release(
            || {
                for keyboard in &keyboards {
                    let state = keyboard
                        .get_key_state()
                        .map_err(|e| format!("cannot read keyboard modifiers: {e}"))?;
                    if MODIFIERS.iter().any(|key| state.contains(*key)) {
                        return Ok(false);
                    }
                }
                Ok(true)
            },
            RELEASE_TIMEOUT,
            RELEASE_POLL,
        )?;

        emit_chord(
            &modifier_keys,
            key,
            |events| device.emit(events),
            STEP_DELAY,
        )
        .map_err(|e| format!("virtual keyboard write failed: {e}"))
    }
}

fn chord_modifiers(mods: u32) -> Result<Vec<KeyCode>, String> {
    // XKB modifier masks used by the Wayland backend and clipboard caller.
    if mods & !(1 | 4) != 0 {
        return Err(format!("unsupported injected modifiers: {mods:#x}"));
    }
    let mut keys = Vec::new();
    if mods & 4 != 0 {
        keys.push(KeyCode::KEY_LEFTCTRL);
    }
    if mods & 1 != 0 {
        keys.push(KeyCode::KEY_LEFTSHIFT);
    }
    Ok(keys)
}

fn wait_for_release(
    mut released: impl FnMut() -> Result<bool, String>,
    timeout: Duration,
    poll: Duration,
) -> Result<(), String> {
    let start = Instant::now();
    let mut was_released = false;
    loop {
        let clear = released()?;
        // Require two clear samples, allowing the compositor to process the
        // physical releases before events arrive from our virtual keyboard.
        if clear && was_released {
            return Ok(());
        }
        was_released = clear;
        if start.elapsed() >= timeout {
            return Err("keyboard modifiers are still held; release them and paste the transcript from the clipboard".into());
        }
        std::thread::sleep(poll);
    }
}

fn emit_chord(
    modifiers: &[KeyCode],
    key: KeyCode,
    mut emit: impl FnMut(&[InputEvent]) -> io::Result<()>,
    delay: Duration,
) -> io::Result<()> {
    let event = |key: KeyCode, value| InputEvent::new(EventType::KEY.0, key.0, value);
    let press = (|| {
        for modifier in modifiers {
            emit(&[event(*modifier, 1)])?;
        }
        std::thread::sleep(delay);
        emit(&[event(key, 1)])?;
        std::thread::sleep(delay);
        Ok(())
    })();
    // Always attempt every release, even after a partially failed write. evdev
    // terminates each emit with SYN_REPORT; do not leave synthetic keys held.
    let mut release = emit(&[event(key, 0)]);
    for modifier in modifiers.iter().rev() {
        let result = emit(&[event(*modifier, 0)]);
        if release.is_ok() {
            release = result;
        }
    }
    press.and(release)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paste_and_copy_press_and_release_in_order() {
        for (mods, key, modifier) in [
            (1, KeyCode::KEY_INSERT, KeyCode::KEY_LEFTSHIFT),
            (4, KeyCode::KEY_C, KeyCode::KEY_LEFTCTRL),
        ] {
            let mut actual = Vec::new();
            emit_chord(
                &chord_modifiers(mods).unwrap(),
                key,
                |events| {
                    actual.extend(events.iter().map(|e| (e.code(), e.value())));
                    Ok(())
                },
                Duration::ZERO,
            )
            .unwrap();
            assert_eq!(
                actual,
                [(modifier.0, 1), (key.0, 1), (key.0, 0), (modifier.0, 0)]
            );
        }
    }

    #[test]
    fn partial_write_failure_still_releases_every_key() {
        for fail_at in 1..=6 {
            let mut calls = 0;
            let mut releases = Vec::new();
            let result = emit_chord(
                &chord_modifiers(5).unwrap(),
                KeyCode::KEY_V,
                |events| {
                    calls += 1;
                    if events[0].value() == 0 {
                        releases.push(events[0].code());
                    }
                    if calls == fail_at {
                        Err(io::Error::other("write failed"))
                    } else {
                        Ok(())
                    }
                },
                Duration::ZERO,
            );
            assert!(result.is_err());
            assert_eq!(
                releases,
                [
                    KeyCode::KEY_V.0,
                    KeyCode::KEY_LEFTSHIFT.0,
                    KeyCode::KEY_LEFTCTRL.0
                ]
            );
        }
    }

    #[test]
    fn waits_for_stable_modifier_release() {
        let mut states = [false, true, false, true, true].into_iter();
        wait_for_release(
            || Ok(states.next().unwrap()),
            Duration::from_secs(1),
            Duration::ZERO,
        )
        .unwrap();
        assert!(states.next().is_none());
    }

    #[test]
    fn held_or_unreadable_modifiers_refuse_injection() {
        assert!(wait_for_release(|| Ok(false), Duration::ZERO, Duration::ZERO).is_err());
        assert_eq!(
            wait_for_release(
                || Err("permission denied".into()),
                Duration::ZERO,
                Duration::ZERO
            ),
            Err("permission denied".into())
        );
        assert!(chord_modifiers(8).is_err());
    }

    #[test]
    #[ignore = "requires access to the host /dev/uinput; creates a device but sends no keys"]
    fn creates_real_virtual_keyboard() {
        let injector = UInputInjector::new().unwrap();
        let mut device = injector.device.lock().unwrap();
        assert!(device.get_syspath().unwrap().exists());
    }
}
