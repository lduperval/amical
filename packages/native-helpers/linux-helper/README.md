# linux-helper

Amical's native helper for Linux (Wayland), implementing the JSON-RPC
stdin/stdout protocol shared with `swift-helper` (macOS) and `windows-helper`
(Windows). See `../LINUX_HELPER_SPEC.md` for the architecture and
`packages/types/src/schemas` for the protocol source of truth.

## Building

```sh
pnpm build        # release build into bin/linux-helper (used by packaging)
pnpm dev          # debug build into bin/linux-helper
```

Requires a Rust toolchain (`rustup default stable`). No native library
dependencies — Wayland and DBus are spoken over pure-Rust bindings.

For automatic paste on GNOME Wayland, build with `cargo build --release
--features uinput` and copy `target/release/linux-helper` to `bin/linux-helper`.
For the desktop package, run `AMICAL_INPUT_METHOD=uinput pnpm make:linux` from
the repository root. See `../../../BUILD.md` for device permissions and testing.

The uinput backend creates a persistent virtual keyboard and waits for physical
modifiers to be released before pasting. It uses Shift+Insert with both clipboard
selections to support standard text fields and terminals. Recording shortcuts
remain configurable through `setShortcuts`. Applications that remap or disable
paste still require manual handling. This helper's clipboard requires Wayland.
The `xtest` and `gnome_ext` features are placeholders and report unavailable.

With `uinput`, active shortcuts also check physical keyboard release every
25 ms. This repairs a missing portal `Deactivated` signal without depending
on key-repeat timing or hardcoding the shortcut. The helper checks the
configured modifier groups and the physical trigger candidates held at
activation, so it does not assume a US keyboard layout. If several non-modifier
keys were held at activation, trigger-only recovery waits until all those
candidates are released; releasing a required modifier still ends the chord.
Unavailable keyboard devices fall back to portal release events. The widget's
stop and dismiss buttons remain available in both recording modes.

## How each capability maps to Wayland

Wayland's security model forbids the global input hooks and cross-client
inspection the other helpers rely on, so every capability goes through a
sanctioned interface and degrades gracefully when the compositor lacks it:

| Capability                                                             | Mechanism                                                                                                                                                                                                                   | Degradation                                                                                                                                                              |
| ---------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Global shortcuts (`setShortcuts`, key events)                          | `org.freedesktop.portal.GlobalShortcuts` (KDE ≥ 5.27, GNOME ≥ 48, Hyprland, …). Portal Activated/Deactivated signals are translated into the `keyDown`/`keyUp` events the desktop expects, echoing the registered keycodes. | `setShortcuts` returns `success: false`; no key events fire.                                                                                                             |
| Clipboard write/read                                                   | `ext-data-control-v1` / `zwlr-data-control-unstable-v1` via wl-clipboard-rs                                                                                                                                                 | Falls back to the `wl-copy`/`wl-paste` CLI tools (package `wl-clipboard`), which support focus-based compositors like Mutter < 48.                                       |
| Paste/copy keystroke injection (`pasteText`, `getSelectedTextViaCopy`) | `uinput` feature: kernel virtual keyboard; default: `zwp_virtual_keyboard_v1` where available                                                                                                                               | `pasteText` leaves the transcript on the clipboard and returns `success: false` with an explanation in `message`; `getSelectedTextViaCopy` reports `selectedText: null`. |
| Recording chrome (`startRecording`/`stopRecording`)                    | Feedback sounds via `paplay`, sink mute via `pactl` (PulseAudio or PipeWire)                                                                                                                                                | Missing tools log to stderr and no-op. Audio _capture_ itself lives in the Electron app, as on the other platforms.                                                      |
| Accessibility context                                                  | none — Wayland clients cannot inspect other clients                                                                                                                                                                         | Always returns `context: null`; permission checks report granted so the desktop skips permission UI.                                                                     |
| `setDraftEnterCapture`                                                 | not possible (no global key interception)                                                                                                                                                                                   | Accepted and stored, no effect.                                                                                                                                          |
| `setAllowInjectedKeys`                                                 | Windows-specific                                                                                                                                                                                                            | Accepted, no-op (same as macOS helper).                                                                                                                                  |

### Keycode space

The desktop's `keycode-map.ts` uses the macOS keycode table on Linux. This
helper never interprets keycodes as physical keys: chords registered through
`setShortcuts` are echoed back verbatim in synthesized key events, so the
desktop's chord matching works unchanged. Keycodes are only translated to
names for portal shortcut descriptions and `preferred_trigger` hints.

### Runtime dependencies (all optional, graceful when absent)

- `wl-clipboard` (`wl-copy`/`wl-paste`) — clipboard fallback on GNOME < 48
- `pulseaudio-utils` (`paplay`/`pactl`) — sounds and system-audio mute
- `xdg-desktop-portal` with a GlobalShortcuts backend — global shortcuts
- With `uinput`: write access to `/dev/uinput` and read access to keyboard devices under `/dev/input/event*` (typically the `input` group; restart the app after logging out and back in).
