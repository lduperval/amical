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

## How each capability maps to Wayland

Wayland's security model forbids the global input hooks and cross-client
inspection the other helpers rely on, so every capability goes through a
sanctioned interface and degrades gracefully when the compositor lacks it:

| Capability | Mechanism | Degradation |
|---|---|---|
| Global shortcuts (`setShortcuts`, key events) | `org.freedesktop.portal.GlobalShortcuts` (KDE ≥ 5.27, GNOME ≥ 48, Hyprland, …). Portal Activated/Deactivated signals are translated into the `keyDown`/`keyUp` events the desktop expects, echoing the registered keycodes. | `setShortcuts` returns `success: false`; no key events fire. |
| Clipboard write/read | `ext-data-control-v1` / `zwlr-data-control-unstable-v1` via wl-clipboard-rs | Falls back to the `wl-copy`/`wl-paste` CLI tools (package `wl-clipboard`), which support focus-based compositors like Mutter < 48. |
| Paste/copy keystroke injection (`pasteText`, `getSelectedTextViaCopy`) | `zwp_virtual_keyboard_v1` (wlroots compositors, KWin) | `pasteText` leaves the transcript on the clipboard and says so in `message`; `getSelectedTextViaCopy` reports `selectedText: null`. |
| Recording chrome (`startRecording`/`stopRecording`) | Feedback sounds via `paplay`, sink mute via `pactl` (PulseAudio or PipeWire) | Missing tools log to stderr and no-op. Audio *capture* itself lives in the Electron app, as on the other platforms. |
| Accessibility context | none — Wayland clients cannot inspect other clients | Always returns `context: null`; permission checks report granted so the desktop skips permission UI. |
| `setDraftEnterCapture` | not possible (no global key interception) | Accepted and stored, no effect. |
| `setAllowInjectedKeys` | Windows-specific | Accepted, no-op (same as macOS helper). |

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
