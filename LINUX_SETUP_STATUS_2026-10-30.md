# New-machine Linux integration diagnosis — 30 October 2026

Host: Ubuntu 26.10, GNOME Shell 51.0, Wayland; Amical 1.12.7 installed.

## Confirmed causes

- `/dev/uinput` is root:root 0600. Amical startup logs report permission denied and `injectionAvailable: false`.
- Keyboard event devices are root:input 0660; laurent is not in input. Logs report physical shortcut release recovery unavailable. This explains loss of the recovery mechanism; actual shortcut reliability must be checked after restoring access.
- The installed Amical GNOME extension was enabled but OUT OF DATE: metadata supported GNOME 45–50. Its D-Bus object was absent. Logs report extension unavailable, wl-clipboard fallback with focus stealing, and inability to position the widget.
- Previous-machine logs show GNOME 50.1, working extension clipboard, and successful uinput paste.

## Completed

- Extension source now declares GNOME 51 compatibility, version 2.
- Use global.stage.context.get_backend() on newer GNOME, preserving the older Clutter API fallback. GNOME 51 removed Clutter.get_default_backend(): https://gjs.guide/extensions/upgrading/gnome-shell-51.html
- Packed and installed the corrected extension for laurent. Existing enable setting was already on. GNOME login required to load updated extension in the real desktop.
- JavaScript syntax check and git diff whitespace check passed.
- Isolated GNOME Shell 51.0 headless session loaded the extension: status reported keyboard=true; clipboard write/read returned the expected text; SendChord(paste) succeeded. PlaceWidget API responded, but no Amical window was present in that isolated session, so actual stacking was not tested.

## Permissions restored after explicit user approval

User approved the input permission changes in chat. Loaded uinput, configured `/etc/modules-load.d/uinput.conf`, installed `/etc/udev/rules.d/99-uinput.rules` (root:input 0660), added laurent to input, and reloaded/triggered udev. Verified group membership and device ownership/mode. Existing graphical processes retain their old groups until logout.

## Pending desktop verification

Log out and back in, then restart Amical. This activates the new input group in the graphical session and loads extension v2; the current Shell still caches v1 as OUT OF DATE. Verify extension status, automatic paste, shortcut release (including releasing the letter before modifiers), and floating widget stacking in the real desktop.

The previously built Debian installer still contains extension v1; corrected v2 is installed separately for this user and will be included in future package builds. Source changes are uncommitted.
