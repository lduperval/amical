import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import { describe, expect, it, vi } from "vitest";

// Exercise the shipped extension with compositor signals, not an Electron
// getBounds mock: native Wayland commits sizes after PlaceWidget returns.
const source = readFileSync(
  new URL(
    "../../../../packages/native-helpers/linux-helper/gnome-extension/amical@amical.ai/extension.js",
    import.meta.url,
  ),
  "utf8",
)
  .replace(/^import .*;$/gm, "")
  .replace("export const API_VERSION", "const API_VERSION")
  .replace("export default class AmicalExtension", "class AmicalExtension");

function setup() {
  let nextId = 0;
  const timers = new Map<number, () => void>();
  const signals = new Map<number, { name: string; callback: () => void }>();
  const GLib = {
    PRIORITY_DEFAULT: 0,
    SOURCE_REMOVE: false,
    timeout_add: (_priority: number, _ms: number, callback: () => void) => {
      timers.set(++nextId, callback);
      return nextId;
    },
    idle_add: (_priority: number, callback: () => void) => {
      timers.set(++nextId, callback);
      return nextId;
    },
    source_remove: (id: number) => timers.delete(id),
    Variant: class {
      constructor(
        public type: string,
        public value: unknown[],
      ) {}
    },
  };
  const Extension = runInNewContext(`${source}\nAmicalExtension`, {
    Extension: class {},
    GLib,
    Meta: { GrabOp: { MOVING: 1, KEYBOARD_MOVING: 257 } },
    Clutter: {},
    logError: (error: Error) => {
      throw error;
    },
  });
  const extension = new Extension();
  extension._widgets = new Map();
  extension._dbus = { emit_signal: vi.fn() };
  let frame = { x: 2200, y: 150, width: 118, height: 34 };
  const window = {
    connect: (name: string, callback: () => void) => {
      signals.set(++nextId, { name, callback });
      return nextId;
    },
    disconnect: (id: number) => signals.delete(id),
    get_frame_rect: () => ({ ...frame }),
    move_frame: vi.fn((_user: boolean, x: number, y: number) => {
      frame = { ...frame, x, y };
      emit("position-changed");
    }),
    hide_from_window_list: vi.fn(),
    show_in_window_list: vi.fn(),
    is_on_all_workspaces: () => true,
    is_above: () => true,
    unmake_above: vi.fn(),
    raise: vi.fn(),
    has_focus: () => false,
  };
  function emit(name: string) {
    for (const signal of [...signals.values()])
      if (signal.name === name) signal.callback();
  }
  function settle() {
    for (const [id, callback] of [...timers]) {
      timers.delete(id);
      callback();
    }
  }
  extension._findWidgetWindow = () => window;
  const place = (x = 2200, y = 150) =>
    extension.PlaceWidget("Amical Widget", x, y, true, true);
  const resize = (width: number, height: number) => {
    // Simulate Mutter gravity moving the surface during an async configure.
    frame = { x: frame.x + 261, y: frame.y + 286, width, height };
    emit("position-changed");
    emit("size-changed");
  };
  return {
    extension,
    window,
    place,
    resize,
    settle,
    emit,
    frame: () => frame,
    timers,
  };
}

describe("GNOME widget placement", () => {
  it("does not save resize/show/placement feedback as a user drag", () => {
    const { extension, place, resize, settle, frame } = setup();
    for (let cycle = 0; cycle < 20; cycle++) {
      place();
      resize(640, 320);
      settle();
      place();
      resize(118, 34);
      settle();
      expect(frame()).toEqual({ x: 2200, y: 150, width: 118, height: 34 });
    }
    expect(extension._dbus.emit_signal).not.toHaveBeenCalled();
  });

  it.each([1, 257, 1025])(
    "persists only an actual completed compositor move (%s)",
    (op) => {
      const { extension, window, place, settle, frame } = setup();
      place();
      extension._onGrabBegin(window, op);
      window.move_frame(true, 2400, 200);
      place(); // A concurrent client request must not undo the drag.
      expect(frame().x).toBe(2400);
      extension._onGrabEnd(window);
      place(); // Nor a request arriving before the settled notification.
      settle();
      expect(extension._dbus.emit_signal).toHaveBeenCalledExactlyOnceWith(
        "WidgetMoved",
        expect.objectContaining({
          value: ["Amical Widget", 2400, 200, 118, 34],
        }),
      );
      expect(extension._widgets.get("Amical Widget")).toMatchObject({
        x: 2400,
        y: 200,
        dragging: false,
      });
    },
  );

  it("hides each adopted Wayland toplevel from the taskbar and restores it on release", () => {
    const { extension, window, place } = setup();
    place();
    expect(window.hide_from_window_list).toHaveBeenCalledOnce();
    extension.ForgetWidget("Amical Widget");
    expect(window.show_in_window_list).toHaveBeenCalledOnce();
  });

  it("cancels deferred work when a widget disappears", () => {
    const { extension, window, place, resize, emit, timers } = setup();
    place();
    resize(640, 320);
    extension._onGrabBegin(window, 1);
    extension._onGrabEnd(window);
    emit("unmanaged");
    expect(timers.size).toBe(0);
    expect(extension._widgets.get("Amical Widget").window).toBeNull();
  });

  it("waits for a second drag to finish when it starts before the first one settles", () => {
    const { extension, window, place, settle } = setup();
    place();
    extension._onGrabBegin(window, 1);
    window.move_frame(true, 2400, 200);
    extension._onGrabEnd(window);
    extension._onGrabBegin(window, 1);
    window.move_frame(true, 2500, 250);
    settle();
    expect(extension._dbus.emit_signal).not.toHaveBeenCalled();
    extension._onGrabEnd(window);
    settle();
    expect(extension._dbus.emit_signal).toHaveBeenCalledExactlyOnceWith(
      "WidgetMoved",
      expect.objectContaining({ value: ["Amical Widget", 2500, 250, 118, 34] }),
    );
  });
});
