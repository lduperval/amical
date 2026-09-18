// Amical Integration — GNOME Shell extension.
//
// GNOME's Mutter does not let ordinary Wayland clients inject keystrokes,
// touch another client's clipboard without keyboard focus, or position and
// stack their own windows. Code running inside GNOME Shell can do all three.
// This extension exposes exactly the operations Amical needs, over the
// session bus, on GNOME Shell's own bus name:
//
//   destination: org.gnome.Shell
//   object path: /org/gnome/Shell/Extensions/Amical
//   interface:   org.gnome.Shell.Extensions.Amical
//
// Security model: the session bus is only reachable by the logged-in user,
// which is the same trust level as the physical keyboard. The interface is
// still kept narrow on purpose — the only key events it can synthesize are a
// fixed set of copy/paste chords (no arbitrary keys, no text typing), and
// window placement only applies to Amical's own windows.

import Clutter from 'gi://Clutter';
import GLib from 'gi://GLib';
import Gio from 'gi://Gio';
import Meta from 'gi://Meta';
import St from 'gi://St';

import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
import * as Config from 'resource:///org/gnome/shell/misc/config.js';

export const API_VERSION = 1;
const OBJECT_PATH = '/org/gnome/Shell/Extensions/Amical';
const ERROR_PREFIX = 'org.gnome.Shell.Extensions.Amical.Error';

// Delay between the individual key transitions of a chord. The focused client
// processes events on its own loop; a short gap keeps modifiers and the key
// from being coalesced or reordered.
const CHORD_STEP_MS = 10;
// Movement notifications are coalesced so a compositor drag emits one signal
// per settled position instead of one per frame.
const MOVE_SETTLE_MS = 120;

// The complete set of chords the extension will ever synthesize.
const CHORDS = {
    'paste': [Clutter.KEY_Control_L, Clutter.KEY_v],
    'paste-primary': [Clutter.KEY_Shift_L, Clutter.KEY_Insert],
    'copy': [Clutter.KEY_Control_L, Clutter.KEY_c],
};

const IFACE_XML = `
<node>
  <interface name="org.gnome.Shell.Extensions.Amical">
    <property name="Version" type="u" access="read"/>
    <method name="GetStatus">
      <arg type="s" direction="out" name="json"/>
    </method>
    <method name="GetClipboardText">
      <arg type="b" direction="in" name="primary"/>
      <arg type="b" direction="out" name="hasText"/>
      <arg type="s" direction="out" name="text"/>
    </method>
    <method name="SetClipboardText">
      <arg type="s" direction="in" name="text"/>
      <arg type="b" direction="in" name="primary"/>
    </method>
    <method name="SendChord">
      <arg type="s" direction="in" name="chord"/>
    </method>
    <method name="PlaceWidget">
      <arg type="s" direction="in" name="title"/>
      <arg type="i" direction="in" name="x"/>
      <arg type="i" direction="in" name="y"/>
      <arg type="b" direction="in" name="above"/>
      <arg type="b" direction="in" name="sticky"/>
      <arg type="b" direction="out" name="found"/>
    </method>
    <method name="ForgetWidget">
      <arg type="s" direction="in" name="title"/>
    </method>
    <method name="GetFocusedWindow">
      <arg type="s" direction="out" name="json"/>
    </method>
    <signal name="WidgetMoved">
      <arg type="s" name="title"/>
      <arg type="i" name="x"/>
      <arg type="i" name="y"/>
      <arg type="i" name="width"/>
      <arg type="i" name="height"/>
    </signal>
  </interface>
</node>`;

function isAmicalWindow(win) {
    const wmClass = (win.get_wm_class() ?? '').toLowerCase();
    const instance = (win.get_wm_class_instance() ?? '').toLowerCase();
    const sandboxed = (win.get_sandboxed_app_id() ?? '').toLowerCase();
    return wmClass.includes('amical') || instance.includes('amical') ||
        sandboxed.includes('amical');
}

function windowInfo(win) {
    const frame = win.get_frame_rect();
    return {
        title: win.get_title() ?? '',
        wmClass: win.get_wm_class() ?? '',
        wmClassInstance: win.get_wm_class_instance() ?? '',
        gtkApplicationId: win.get_gtk_application_id() ?? '',
        sandboxedAppId: win.get_sandboxed_app_id() ?? '',
        pid: win.get_pid(),
        clientType: win.get_client_type() === Meta.WindowClientType.WAYLAND ? 'wayland' : 'x11',
        frame: {x: frame.x, y: frame.y, width: frame.width, height: frame.height},
    };
}

export default class AmicalExtension extends Extension {
    enable() {
        this._keyboard = null;
        this._chordQueue = [];
        this._chordRunning = false;
        // title -> {x, y, above, sticky, window, signals, settleTimer}
        this._widgets = new Map();
        this._lastForeignFocus = null;

        try {
            const seat = Clutter.get_default_backend().get_default_seat();
            this._keyboard = seat.create_virtual_device(
                Clutter.InputDeviceType.KEYBOARD_DEVICE);
        } catch (e) {
            logError(e, 'Amical: could not create the virtual keyboard');
        }

        this._dbus = Gio.DBusExportedObject.wrapJSObject(IFACE_XML, this);
        this._dbus.export(Gio.DBus.session, OBJECT_PATH);

        this._windowCreatedId = global.display.connect('window-created',
            (_display, win) => this._onWindowCreated(win));
        this._focusId = global.display.connect('notify::focus-window',
            () => this._onFocusChanged());
        this._onFocusChanged();
    }

    disable() {
        if (this._windowCreatedId) {
            global.display.disconnect(this._windowCreatedId);
            this._windowCreatedId = 0;
        }
        if (this._focusId) {
            global.display.disconnect(this._focusId);
            this._focusId = 0;
        }
        for (const title of [...this._widgets.keys()])
            this._releaseWidget(title, true);
        this._widgets.clear();
        if (this._dbus) {
            this._dbus.unexport();
            this._dbus = null;
        }
        this._chordQueue = [];
        this._chordRunning = false;
        this._keyboard = null;
        this._lastForeignFocus = null;
    }

    // ------------------------------------------------------------------
    // D-Bus API
    // ------------------------------------------------------------------

    get Version() {
        return API_VERSION;
    }

    GetStatus() {
        return JSON.stringify({
            version: API_VERSION,
            shellVersion: Config.PACKAGE_VERSION,
            keyboard: this._keyboard !== null,
            chords: Object.keys(CHORDS),
            widgets: [...this._widgets.entries()].map(([title, w]) => ({
                title, x: w.x, y: w.y, above: w.above, sticky: w.sticky,
                tracked: w.window !== null,
                window: w.window ? windowInfo(w.window) : null,
                isAbove: w.window ? w.window.is_above() : null,
                onAllWorkspaces: w.window ? w.window.is_on_all_workspaces() : null,
            })),
        });
    }

    GetClipboardTextAsync([primary], invocation) {
        const type = primary ? St.ClipboardType.PRIMARY : St.ClipboardType.CLIPBOARD;
        St.Clipboard.get_default().get_text(type, (_clipboard, text) => {
            const hasText = typeof text === 'string';
            invocation.return_value(new GLib.Variant('(bs)', [hasText, hasText ? text : '']));
        });
    }

    SetClipboardText(text, primary) {
        const type = primary ? St.ClipboardType.PRIMARY : St.ClipboardType.CLIPBOARD;
        St.Clipboard.get_default().set_text(type, text);
    }

    SendChordAsync([chord], invocation) {
        const keyvals = CHORDS[chord];
        if (!keyvals) {
            invocation.return_dbus_error(`${ERROR_PREFIX}.UnknownChord`,
                `Unknown chord '${chord}'. Supported: ${Object.keys(CHORDS).join(', ')}`);
            return;
        }
        if (!this._keyboard) {
            invocation.return_dbus_error(`${ERROR_PREFIX}.NoKeyboard`,
                'The virtual keyboard could not be created');
            return;
        }
        const steps = [
            ...keyvals.map(k => [k, Clutter.KeyState.PRESSED]),
            ...[...keyvals].reverse().map(k => [k, Clutter.KeyState.RELEASED]),
        ];
        this._chordQueue.push({steps, invocation});
        this._pumpChords();
    }

    PlaceWidget(title, x, y, above, sticky) {
        if (!title)
            throw new Error('title must not be empty');
        const entry = this._widgets.get(title) ?? {
            window: null, signals: [], settleTimer: 0,
        };
        Object.assign(entry, {x, y, above, sticky});
        this._widgets.set(title, entry);

        const win = entry.window ?? this._findWidgetWindow(title);
        if (!win)
            return false;
        this._adoptWindow(title, entry, win);
        return true;
    }

    ForgetWidget(title) {
        this._releaseWidget(title, true);
        this._widgets.delete(title);
    }

    GetFocusedWindow() {
        const win = global.display.get_focus_window();
        return JSON.stringify(win ? windowInfo(win) : null);
    }

    // ------------------------------------------------------------------
    // Key injection
    // ------------------------------------------------------------------

    _pumpChords() {
        if (this._chordRunning)
            return;
        const next = this._chordQueue.shift();
        if (!next)
            return;
        this._chordRunning = true;
        const {steps, invocation} = next;
        let index = 0;
        const run = () => {
            if (!this._keyboard) {
                this._chordRunning = false;
                invocation.return_dbus_error(`${ERROR_PREFIX}.NoKeyboard`,
                    'The virtual keyboard went away');
                return GLib.SOURCE_REMOVE;
            }
            const [keyval, state] = steps[index++];
            this._keyboard.notify_keyval(GLib.get_monotonic_time(), keyval, state);
            if (index < steps.length) {
                GLib.timeout_add(GLib.PRIORITY_DEFAULT, CHORD_STEP_MS, run);
            } else {
                this._chordRunning = false;
                invocation.return_value(null);
                this._pumpChords();
            }
            return GLib.SOURCE_REMOVE;
        };
        run();
    }

    // ------------------------------------------------------------------
    // Widget window management
    // ------------------------------------------------------------------

    _findWidgetWindow(title) {
        for (const actor of global.get_window_actors()) {
            const win = actor.get_meta_window();
            if (win && win.get_title() === title && isAmicalWindow(win))
                return win;
        }
        return null;
    }

    _onWindowCreated(win) {
        if (!isAmicalWindow(win))
            return;
        const tryAdopt = () => {
            const title = win.get_title();
            const entry = title ? this._widgets.get(title) : undefined;
            if (entry && entry.window !== win)
                this._adoptWindow(title, entry, win);
        };
        // The title usually arrives with the first commit, but not always
        // before window-created; watch it and retry once the window is shown.
        const titleId = win.connect('notify::title', tryAdopt);
        const shownId = win.connect('shown', tryAdopt);
        const unmanagedId = win.connect('unmanaged', () => {
            win.disconnect(titleId);
            win.disconnect(shownId);
            win.disconnect(unmanagedId);
        });
        tryAdopt();
    }

    _adoptWindow(title, entry, win) {
        if (entry.window && entry.window !== win)
            this._releaseWidget(title, false);
        if (entry.window !== win) {
            entry.window = win;
            entry.signals = [
                win.connect('position-changed', () => this._scheduleMoved(title)),
                win.connect('size-changed', () => this._scheduleMoved(title)),
                win.connect('focus', () => this._restoreForeignFocus()),
                win.connect('unmanaged', () => this._releaseWidget(title, false)),
            ];
        }
        try {
            if (entry.sticky && !win.is_on_all_workspaces())
                win.stick();
            if (entry.above && !win.is_above())
                win.make_above();
            else if (!entry.above && win.is_above())
                win.unmake_above();
            const frame = win.get_frame_rect();
            if (frame.x !== entry.x || frame.y !== entry.y)
                win.move_frame(false, entry.x, entry.y);
            win.raise();
            if (win.has_focus())
                this._restoreForeignFocus();
        } catch (e) {
            logError(e, `Amical: could not place widget '${title}'`);
        }
    }

    _releaseWidget(title, unmakeAbove) {
        const entry = this._widgets.get(title);
        if (!entry)
            return;
        if (entry.settleTimer) {
            GLib.source_remove(entry.settleTimer);
            entry.settleTimer = 0;
        }
        const win = entry.window;
        if (win) {
            for (const id of entry.signals) {
                try {
                    win.disconnect(id);
                } catch (_e) {
                    // The window is already gone.
                }
            }
            if (unmakeAbove) {
                try {
                    if (win.is_above())
                        win.unmake_above();
                } catch (_e) {
                    // Nothing to undo on a dead window.
                }
            }
        }
        entry.window = null;
        entry.signals = [];
    }

    _scheduleMoved(title) {
        const entry = this._widgets.get(title);
        if (!entry)
            return;
        if (entry.settleTimer)
            GLib.source_remove(entry.settleTimer);
        entry.settleTimer = GLib.timeout_add(GLib.PRIORITY_DEFAULT, MOVE_SETTLE_MS, () => {
            entry.settleTimer = 0;
            const win = entry.window;
            if (win && this._dbus) {
                const frame = win.get_frame_rect();
                // Remember where the user put it so a re-created toplevel
                // (Electron re-maps the window on every show) lands there.
                entry.x = frame.x;
                entry.y = frame.y;
                this._dbus.emit_signal('WidgetMoved', new GLib.Variant('(siiii)',
                    [title, frame.x, frame.y, frame.width, frame.height]));
            }
            return GLib.SOURCE_REMOVE;
        });
    }

    // The widget is a non-interactive overlay: if the compositor hands it
    // keyboard focus (Wayland clients cannot decline focus), give focus back
    // to the window the user was working in so dictation keeps landing there.
    _onFocusChanged() {
        const win = global.display.get_focus_window();
        if (win && !isAmicalWindow(win))
            this._lastForeignFocus = win;
    }

    _restoreForeignFocus() {
        const previous = this._lastForeignFocus;
        if (!previous)
            return;
        GLib.idle_add(GLib.PRIORITY_DEFAULT, () => {
            try {
                if (previous.get_workspace() && !previous.minimized)
                    previous.activate(global.get_current_time());
            } catch (_e) {
                // The previous window may have been closed meanwhile.
            }
            return GLib.SOURCE_REMOVE;
        });
    }
}
