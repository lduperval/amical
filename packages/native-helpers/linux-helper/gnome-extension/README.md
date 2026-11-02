# Amical Integration (GNOME Shell extension)

GNOME's Mutter does not let an ordinary Wayland application inject keystrokes,
read or write the clipboard without keyboard focus, or position and stack its
own windows. Amical therefore ships a small GNOME Shell extension that does
those three things on Amical's behalf. It is optional: without it Amical falls
back to the `uinput` virtual keyboard (needs `/dev/uinput` access) and to
`wl-copy`/`wl-paste`, which briefly steal focus from the target window.

## What it does

| Method (interface `org.gnome.Shell.Extensions.Amical`)         | Purpose                                                                                                                                                              |
| -------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `GetStatus() -> s`                                             | JSON: API version, GNOME Shell version, whether the virtual keyboard exists, tracked widgets.                                                                        |
| `GetClipboardText(b primary) -> (b hasText, s text)`           | Read the clipboard or primary selection from inside the shell (no focus change).                                                                                     |
| `SetClipboardText(s text, b primary)`                          | Write the clipboard or primary selection (no focus change).                                                                                                          |
| `SendChord(s chord)`                                           | Synthesize one of `paste` (Ctrl+V), `paste-primary` (Shift+Insert) or `copy` (Ctrl+C) with a Clutter virtual keyboard. No other keys can be sent.                    |
| `PlaceWidget(s title, i x, i y, b above, b sticky) -> b found` | Keep the Amical window with that title above other windows, on every workspace, at the given position. Re-applied automatically when Electron re-creates the window. |
| `ForgetWidget(s title)`                                        | Stop managing that window.                                                                                                                                           |
| `GetFocusedWindow() -> s`                                      | JSON with the focused window's title, WM class, app id and PID, or `null`.                                                                                           |
| signal `WidgetMoved(s title, i x, i y, i width, i height)`     | Emitted after the user drags a managed widget.                                                                                                                       |

Object path: `/org/gnome/Shell/Extensions/Amical`, on GNOME Shell's own bus
name `org.gnome.Shell` (session bus).

Security: the session bus is only reachable by the logged-in user, the same
trust level as the physical keyboard. The interface stays narrow anyway: the
chord list is fixed, there is no "type this text" method, and window placement
only applies to windows whose WM class contains `amical`.

## Install

```bash
# From an installed Amical package:
/usr/lib/amical-desktop/resources/bin/gnome-extension/install-amical-gnome-extension.sh
# Or from a checkout:
node packages/native-helpers/linux-helper/scripts/pack-gnome-extension.mjs /tmp/amical-ext
/tmp/amical-ext/install-amical-gnome-extension.sh
```

Then **log out and back in** (GNOME Shell on Wayland only loads new
extensions at login), and verify:

```bash
gnome-extensions info amical@amical.ai          # State: ACTIVE
busctl --user call org.gnome.Shell /org/gnome/Shell/Extensions/Amical \
  org.gnome.Shell.Extensions.Amical GetStatus
```

Restart Amical afterwards. Its log (`~/.config/Amical/logs/amical.log`) prints
`GNOME Shell integration: available` from the Linux helper when the extension
answers.

## Development

- `node --input-type=module --check < amical@amical.ai/extension.js` checks the syntax.
- `journalctl --user -f -o cat /usr/bin/gnome-shell` shows extension errors.
- A nested shell (`dbus-run-session -- gnome-shell --nested --wayland`) loads
  the extension without logging out; point clients at its `WAYLAND_DISPLAY`
  and `DBUS_SESSION_BUS_ADDRESS`.
