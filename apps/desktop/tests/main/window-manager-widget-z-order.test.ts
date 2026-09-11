import { EventEmitter } from "node:events";
import {
  afterAll,
  afterEach,
  beforeEach,
  describe,
  expect,
  it,
  vi,
} from "vitest";
import { WindowManager } from "@main/core/window-manager";
import { screen } from "electron";

const originalPlatform = process.platform;

beforeEach(() => {
  Object.defineProperty(process, "platform", {
    configurable: true,
    value: "win32",
  });
});

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

afterAll(() => {
  Object.defineProperty(process, "platform", {
    configurable: true,
    value: originalPlatform,
  });
});

const createManager = () =>
  Object.create(WindowManager.prototype) as WindowManager;

describe("WindowManager widget z-order recovery", () => {
  it("moves a newly shown widget to the top on Windows", () => {
    const manager = createManager();
    const showInactive = vi.fn();
    const moveTop = vi.fn();
    Reflect.set(manager, "widgetWindow", {
      isDestroyed: vi.fn(() => false),
      isVisible: vi.fn(() => true),
      showInactive,
      moveTop,
    });

    manager.showWidget();

    expect(showInactive).toHaveBeenCalledOnce();
    expect(moveTop).toHaveBeenCalledOnce();
  });

  it("moves only an existing visible widget to the top", () => {
    const manager = createManager();
    const moveTop = vi.fn();
    const widgetWindow = {
      isDestroyed: vi.fn(() => false),
      isVisible: vi.fn(() => true),
      moveTop,
    };
    Reflect.set(manager, "widgetWindow", widgetWindow);

    manager.reassertWidgetZOrder();

    expect(moveTop).toHaveBeenCalledOnce();

    widgetWindow.isVisible.mockReturnValue(false);
    manager.reassertWidgetZOrder();

    widgetWindow.isVisible.mockReturnValue(true);
    widgetWindow.isDestroyed.mockReturnValue(true);
    manager.reassertWidgetZOrder();

    expect(moveTop).toHaveBeenCalledOnce();
  });

  it("does not change z-order outside Windows", () => {
    Object.defineProperty(process, "platform", {
      configurable: true,
      value: "darwin",
    });
    const manager = createManager();
    const moveTop = vi.fn();
    Reflect.set(manager, "widgetWindow", {
      isDestroyed: vi.fn(() => false),
      isVisible: vi.fn(() => true),
      moveTop,
    });

    manager.reassertWidgetZOrder();

    expect(moveTop).not.toHaveBeenCalled();
  });

  it("uses an input shape so the idle Linux pill is directly clickable", () => {
    Object.defineProperty(process, "platform", {
      configurable: true,
      value: "linux",
    });
    const manager = createManager();
    const setIgnoreMouseEvents = vi.fn();
    const setShape = vi.fn();
    Reflect.set(manager, "widgetWindow", {
      isDestroyed: vi.fn(() => false),
      getBounds: vi.fn(() => ({ x: 100, y: 200, width: 640, height: 320 })),
      setIgnoreMouseEvents,
      setShape,
    });

    manager.setWidgetIgnoreMouseEvents(true);

    expect(setShape).toHaveBeenCalledWith([
      { x: 264, y: 282, width: 112, height: 38 },
    ]);
    expect(setIgnoreMouseEvents).toHaveBeenCalledWith(false);

    manager.setWidgetIgnoreMouseEvents(false);

    expect(setShape).toHaveBeenLastCalledWith([
      { x: 0, y: 0, width: 640, height: 320 },
    ]);
  });

  it("keeps the Linux widget clear of a bottom dock", () => {
    Object.defineProperty(process, "platform", {
      configurable: true,
      value: "linux",
    });
    const manager = createManager();
    Reflect.set(manager, "widgetEdgeInset", 0);

    const bounds = Reflect.get(manager, "getWidgetDefaultBounds").call(
      manager,
      { x: 0, y: 0, width: 1920, height: 1032 },
    );

    expect(bounds).toEqual({ x: 640, y: 688, width: 640, height: 320 });
  });

  it("follows the cursor after it settles on another display", () => {
    vi.useFakeTimers();
    Object.defineProperty(process, "platform", {
      configurable: true,
      value: "linux",
    });
    const manager = createManager();
    Reflect.set(manager, "widgetEdgeInset", 0);
    Reflect.set(manager, "widgetDisplayId", 1);
    const setBounds = vi.fn();
    Reflect.set(manager, "widgetWindow", {
      isDestroyed: vi.fn(() => false),
      setBounds,
    });
    vi.mocked(screen.getCursorScreenPoint).mockReturnValue({ x: 2100, y: 500 });
    vi.mocked(screen.getDisplayNearestPoint).mockReturnValue({
      id: 2,
      workArea: { x: 1920, y: 0, width: 2560, height: 1400 },
    } as Electron.Display);

    Reflect.get(manager, "startCursorPolling").call(manager);
    vi.advanceTimersByTime(200);
    expect(setBounds).not.toHaveBeenCalled();

    vi.advanceTimersByTime(200);
    expect(setBounds).toHaveBeenCalledOnce();
    expect(setBounds).toHaveBeenCalledWith({
      x: 2880,
      y: 1056,
      width: 640,
      height: 320,
    });
    expect(Reflect.get(manager, "widgetDisplayId")).toBe(2);

    clearInterval(Reflect.get(manager, "cursorPollingInterval"));
  });

  it("persists a middle-button drag as a monitor-relative position", async () => {
    Object.defineProperty(process, "platform", {
      configurable: true,
      value: "linux",
    });
    const manager = createManager();
    Reflect.set(manager, "widgetEdgeInset", 0);
    const updateSettings = vi.fn().mockResolvedValue(undefined);
    Reflect.set(manager, "settingsService", {
      getUISettings: vi.fn().mockResolvedValue({ theme: "system" }),
      updateSettings,
    });

    let bounds = { x: 640, y: 696, width: 640, height: 320 };
    const setBounds = vi.fn((next: Electron.Rectangle) => {
      bounds = next;
    });
    Reflect.set(manager, "widgetWindow", {
      isDestroyed: vi.fn(() => false),
      getBounds: vi.fn(() => bounds),
      setBounds,
    });
    const display = {
      id: 1,
      workArea: { x: 0, y: 0, width: 1920, height: 1040 },
    } as Electron.Display;
    vi.mocked(screen.getDisplayNearestPoint).mockReturnValue(display);

    manager.beginWidgetDrag({ x: 100, y: 100 });
    manager.updateWidgetDrag({ x: 300, y: 0 });
    await manager.endWidgetDrag({ x: 300, y: 0 });

    expect(bounds).toEqual({ x: 840, y: 596, width: 640, height: 320 });
    expect(updateSettings).toHaveBeenCalledOnce();
    const saved = updateSettings.mock.calls[0][0].ui.widgetPosition;
    expect(saved.xRatio).toBeCloseTo(1160 / 1920);
    expect(saved.yRatio).toBeCloseTo(896 / 1040);

    const restored = Reflect.get(manager, "getWidgetBoundsForWorkArea").call(
      manager,
      { x: 1920, y: 0, width: 2560, height: 1400 },
    );
    expect(restored).toEqual({
      x: 3147,
      y: 906,
      width: 640,
      height: 320,
    });
  });

  it("reasserts only after the notes window has closed", () => {
    const manager = createManager();
    const notesWindow = Object.assign(new EventEmitter(), {
      isDestroyed: vi.fn(() => false),
    });
    const close = vi.fn();
    Reflect.set(manager, "notesWindowController", {
      getWindow: () => notesWindow,
      close,
    });
    const reassert = vi
      .spyOn(manager, "reassertWidgetZOrder")
      .mockImplementation(() => undefined);

    manager.closeNotesWindow();

    expect(close).toHaveBeenCalledOnce();
    expect(reassert).not.toHaveBeenCalled();

    notesWindow.emit("closed");

    expect(reassert).toHaveBeenCalledOnce();
  });

  it("reasserts when an existing widget window is ensured", async () => {
    const manager = createManager();
    Reflect.set(manager, "widgetWindow", {
      isDestroyed: vi.fn(() => false),
    });
    const reassert = vi
      .spyOn(manager, "reassertWidgetZOrder")
      .mockImplementation(() => undefined);

    await manager.ensureWidgetWindow();

    expect(reassert).toHaveBeenCalledOnce();
  });
});
