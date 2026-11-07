import {
  afterAll,
  afterEach,
  beforeEach,
  describe,
  expect,
  it,
  vi,
} from "vitest";
import { WIDGET_WINDOW_TITLE, WindowManager } from "@main/core/window-manager";
import type { SettingsService } from "@/services/settings-service";
import { app, screen } from "electron";

const originalPlatform = process.platform;
const defaultDisplay = screen.getPrimaryDisplay();

beforeEach(() => {
  vi.mocked(screen.getAllDisplays).mockReturnValue([defaultDisplay]);
  vi.mocked(screen.getPrimaryDisplay).mockReturnValue(defaultDisplay);
  vi.mocked(screen.getDisplayNearestPoint).mockReturnValue(defaultDisplay);
  Object.defineProperty(process, "platform", {
    configurable: true,
    value: "linux",
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

// Reuses the prototype-only construction used by the z-order tests: the
// Live layer is the only public constructor, and these tests exercise the
// placement methods in isolation.
const createManager = (mode: "wayland" | "x11") => {
  const manager = Object.create(WindowManager.prototype) as WindowManager;
  const settingsService = {
    getUISettings: vi.fn(async () => ({ theme: "system" })),
    updateSettings: vi.fn(async () => undefined),
  };
  Reflect.set(manager, "settingsService", settingsService);
  Reflect.set(manager, "linuxWindowingMode", mode);
  Reflect.set(manager, "widgetPosition", null);
  Reflect.set(manager, "widgetDisplayId", null);
  Reflect.set(manager, "cursorDisplayCandidateId", null);
  // Instance field initializers do not run for Object.create; the Linux
  // build has no edge inset.
  Reflect.set(manager, "widgetEdgeInset", 0);
  const widgetWindow = {
    isDestroyed: vi.fn(() => false),
    isVisible: vi.fn(() => true),
    showInactive: vi.fn(),
    setBounds: vi.fn(),
    setSize: vi.fn(),
    setMinimumSize: vi.fn(),
    setMaximumSize: vi.fn(),
    setShape: vi.fn(),
    setIgnoreMouseEvents: vi.fn(),
    getBounds: vi.fn(() => ({ x: 0, y: 0, width: 640, height: 320 })),
  };
  Reflect.set(manager, "widgetWindow", widgetWindow);
  return {
    manager,
    widgetWindow,
    settingsService: settingsService as unknown as SettingsService & {
      updateSettings: ReturnType<typeof vi.fn>;
    },
  };
};

describe("WindowManager on native Wayland", () => {
  it.each([
    { savedDisplayId: undefined, expectedDisplayId: 2, expectedX: 2821 },
    { savedDisplayId: 1, expectedDisplayId: 1, expectedX: 901 },
  ])(
    "restores monitor $expectedDisplayId at startup (saved: $savedDisplayId)",
    async ({ savedDisplayId, expectedDisplayId, expectedX }) => {
      const { manager, settingsService } = createManager("wayland");
      const left = {
        id: 1,
        workArea: { x: 0, y: 0, width: 1920, height: 1040 },
      } as Electron.Display;
      const right = {
        id: 2,
        workArea: { x: 1920, y: 0, width: 1920, height: 1040 },
      } as Electron.Display;
      vi.mocked(screen.getAllDisplays).mockReturnValue([left, right]);
      vi.mocked(screen.getPrimaryDisplay).mockReturnValue(right);
      vi.mocked(screen.getDisplayNearestPoint).mockReturnValue(left);
      vi.mocked(settingsService.getUISettings).mockResolvedValue({
        theme: "system",
        widgetPosition: { displayId: savedDisplayId, xRatio: 0.5, yRatio: 0.5 },
      });
      const place = vi.fn(async () => ({ success: true, found: true }));
      manager.setLinuxWidgetPlacer({ place });
      await manager.createWidgetWindow();
      const window = Reflect.get(manager, "widgetWindow");
      manager.showWidget();
      expect(place).toHaveBeenCalledWith(
        expect.objectContaining({ x: expectedX }),
      );
      window.emit("moved");
      expect(Reflect.get(manager, "widgetDisplayId")).toBe(expectedDisplayId);
    },
  );

  it("asks the compositor to place the widget when it is shown", async () => {
    const { manager, widgetWindow } = createManager("wayland");
    const place = vi.fn(async () => ({ success: true, found: true }));
    manager.setLinuxWidgetPlacer({ place });

    manager.showWidget();

    expect(widgetWindow.showInactive).toHaveBeenCalledOnce();
    expect(widgetWindow.setBounds).not.toHaveBeenCalled();
    await vi.waitFor(() => expect(place).toHaveBeenCalledOnce());
    // Compact window at bottom-centre, with 24px dock clearance.
    expect(place).toHaveBeenCalledWith({
      title: WIDGET_WINDOW_TITLE,
      x: 901,
      y: 982,
      above: true,
      sticky: true,
    });
  });

  it("keeps a compact surface on hover and restores it after larger content closes", () => {
    const { manager, widgetWindow } = createManager("wayland");
    let bounds = { x: 0, y: 0, width: 118, height: 34 };
    widgetWindow.getBounds.mockImplementation(() => bounds);
    widgetWindow.setSize.mockImplementation((width, height) => {
      bounds = { ...bounds, width, height };
    });
    const place = vi.fn(async () => ({ success: true, found: true }));
    manager.setLinuxWidgetPlacer({ place });

    manager.setWidgetIgnoreMouseEvents(false, false);
    expect(widgetWindow.setSize).not.toHaveBeenCalled();

    manager.setWidgetIgnoreMouseEvents(false, true);
    expect(widgetWindow.setSize).toHaveBeenLastCalledWith(640, 320);
    expect(place).toHaveBeenLastCalledWith(
      expect.objectContaining({ x: 640, y: 696 }),
    );

    // A toast closing while the mouse still hovers must also shrink the surface.
    manager.setWidgetIgnoreMouseEvents(false, false);
    expect(widgetWindow.setSize).toHaveBeenLastCalledWith(118, 34);
    expect(place).toHaveBeenLastCalledWith(
      expect.objectContaining({ x: 901, y: 982 }),
    );
    expect(widgetWindow.setShape).not.toHaveBeenCalled();
    expect(widgetWindow.setIgnoreMouseEvents).toHaveBeenLastCalledWith(false);
  });

  it("sizes the Wayland surface to the measured pill plus 5px per side", () => {
    const { manager, widgetWindow } = createManager("wayland");
    manager.setWidgetControlSize({ width: 108, height: 24 });
    manager.setWidgetControlSize({ width: 136, height: 24 });
    expect(widgetWindow.setSize).toHaveBeenLastCalledWith(146, 34);
    manager.setWidgetControlSize({ width: 96, height: 30 });
    expect(widgetWindow.setSize).toHaveBeenLastCalledWith(106, 40);
  });

  it("updates native size constraints even when Electron already reports the compact size", () => {
    const { manager, widgetWindow } = createManager("wayland");
    widgetWindow.getBounds.mockReturnValue({
      x: 0,
      y: 0,
      width: 118,
      height: 34,
    });
    manager.setLinuxWidgetPlacer({
      place: vi.fn(async () => ({ success: true, found: true })),
    });
    manager.showWidget();
    expect(widgetWindow.setSize).not.toHaveBeenCalled();
    expect(widgetWindow.setMinimumSize.mock.calls).toEqual([
      [1, 1],
      [118, 34],
    ]);
    expect(widgetWindow.setMaximumSize).toHaveBeenCalledWith(118, 34);
    manager.setWidgetIgnoreMouseEvents(false, true);
    expect(widgetWindow.setMaximumSize).toHaveBeenLastCalledWith(640, 320);
    expect(widgetWindow.setMinimumSize).toHaveBeenLastCalledWith(640, 320);
    manager.setWidgetIgnoreMouseEvents(false, false);
    expect(widgetWindow.setMaximumSize).toHaveBeenLastCalledWith(118, 34);
    expect(widgetWindow.setMinimumSize).toHaveBeenLastCalledWith(118, 34);
  });

  it("remembers measured dimensions during expanded content and applies them on collapse", () => {
    const { manager, widgetWindow } = createManager("wayland");
    manager.setWidgetIgnoreMouseEvents(false, true);
    widgetWindow.setSize.mockClear();
    manager.setWidgetControlSize({ width: 128, height: 24 });
    expect(widgetWindow.setSize).not.toHaveBeenCalled();
    manager.setWidgetIgnoreMouseEvents(true, false);
    expect(widgetWindow.setSize).toHaveBeenLastCalledWith(138, 34);
  });

  it("keeps using setBounds on XWayland", () => {
    const { manager, widgetWindow } = createManager("x11");
    const place = vi.fn(async () => ({ success: true, found: true }));
    manager.setLinuxWidgetPlacer({ place });

    manager.handleDisplayChange("test");

    expect(widgetWindow.setBounds).toHaveBeenCalledOnce();
    expect(place).not.toHaveBeenCalled();
  });

  it("ignores main-process drags on Wayland (the compositor moves the window)", async () => {
    const { manager, widgetWindow, settingsService } = createManager("wayland");

    manager.beginWidgetDrag({ x: 10, y: 10 });
    manager.updateWidgetDrag({ x: 50, y: 50 });
    await manager.endWidgetDrag({ x: 50, y: 50 });

    expect(widgetWindow.setBounds).not.toHaveBeenCalled();
    expect(settingsService.updateSettings).not.toHaveBeenCalled();
  });

  it("persists the anchor reported by the compositor after a drag", async () => {
    const { manager, settingsService } = createManager("wayland");

    await manager.handleExternalWidgetMove({
      title: WIDGET_WINDOW_TITLE,
      x: 320,
      y: 200,
      width: 640,
      height: 320,
    });

    // Anchor = window centre-x, bottom minus the 8px margin and half the
    // 24px idle pill: (640, 503) over the 1920x1040 work area.
    expect(settingsService.updateSettings).toHaveBeenCalledWith({
      ui: {
        theme: "system",
        widgetPosition: {
          displayId: 1,
          xRatio: 640 / 1920,
          yRatio: 503 / 1040,
        },
      },
    });
  });

  it("ignores moves of other Amical windows", async () => {
    const { manager, settingsService } = createManager("wayland");

    await manager.handleExternalWidgetMove({
      title: "Amical Notes",
      x: 1,
      y: 1,
      width: 10,
      height: 10,
    });

    expect(settingsService.updateSettings).not.toHaveBeenCalled();
  });

  it("does not snap a compositor drag back to the cursor's bogus origin", async () => {
    vi.useFakeTimers();
    const { manager } = createManager("wayland");
    const right = {
      id: 2,
      workArea: { x: 1920, y: 0, width: 1920, height: 1040 },
    } as Electron.Display;
    vi.mocked(screen.getDisplayNearestPoint).mockReturnValue(right);
    await manager.handleExternalWidgetMove({
      title: WIDGET_WINDOW_TITLE,
      x: 2400,
      y: 500,
      width: 640,
      height: 320,
    });
    const place = vi.fn(async () => ({ success: true, found: true }));
    manager.setLinuxWidgetPlacer({ place });
    vi.mocked(screen.getCursorScreenPoint).mockReturnValue({ x: 0, y: 0 });
    Reflect.get(manager, "startCursorPolling").call(manager);
    vi.advanceTimersByTime(1000);
    expect(Reflect.get(manager, "widgetDisplayId")).toBe(2);
    expect(place).not.toHaveBeenCalled();
  });

  it("keeps the dragged monitor during display updates and widget shows", () => {
    const { manager } = createManager("wayland");
    const left = {
      id: 1,
      workArea: { x: 0, y: 0, width: 1920, height: 1040 },
    } as Electron.Display;
    const right = {
      id: 2,
      workArea: { x: 1920, y: 0, width: 1920, height: 1040 },
    } as Electron.Display;
    vi.mocked(screen.getAllDisplays).mockReturnValue([left, right]);
    vi.mocked(screen.getPrimaryDisplay).mockReturnValue(right);
    vi.mocked(screen.getDisplayNearestPoint).mockReturnValue(left);
    Reflect.set(manager, "widgetDisplayId", 2);
    const place = vi.fn(async () => ({ success: true, found: true }));
    manager.setLinuxWidgetPlacer({ place });
    manager.handleDisplayChange("display-metrics-changed");
    manager.showWidget();
    expect(place).toHaveBeenCalledTimes(2);
    expect(place).toHaveBeenLastCalledWith(
      expect.objectContaining({ x: 2821, above: true }),
    );
    expect(Reflect.get(manager, "widgetDisplayId")).toBe(2);
  });

  it("does not infer Wayland monitor changes from client-local focus bounds", () => {
    const { manager } = createManager("wayland");
    vi.mocked(app.on).mockClear();
    Reflect.get(manager, "setupFocusBasedDisplayDetection").call(manager);
    expect(app.on).not.toHaveBeenCalledWith(
      "browser-window-focus",
      expect.any(Function),
    );
  });

  it("remembers a monitor change even when its relative position is unchanged", async () => {
    const { manager, settingsService } = createManager("wayland");
    Reflect.set(manager, "widgetPosition", {
      displayId: 1,
      xRatio: 0.5,
      yRatio: 0.5,
    });
    vi.mocked(screen.getDisplayNearestPoint).mockReturnValue({
      id: 2,
      workArea: { x: 1920, y: 0, width: 1920, height: 1040 },
    } as Electron.Display);
    await manager.handleExternalWidgetMove({
      title: WIDGET_WINDOW_TITLE,
      x: 2560,
      y: 217,
      width: 640,
      height: 320,
    });
    expect(settingsService.updateSettings).toHaveBeenCalledWith({
      ui: {
        theme: "system",
        widgetPosition: { displayId: 2, xRatio: 0.5, yRatio: 0.5 },
      },
    });
  });
});
