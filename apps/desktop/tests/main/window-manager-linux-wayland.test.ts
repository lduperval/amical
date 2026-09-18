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

const originalPlatform = process.platform;

beforeEach(() => {
  Object.defineProperty(process, "platform", {
    configurable: true,
    value: "linux",
  });
});

afterEach(() => {
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
  it("asks the compositor to place the widget when it is shown", async () => {
    const { manager, widgetWindow } = createManager("wayland");
    const place = vi.fn(async () => ({ success: true, found: true }));
    manager.setLinuxWidgetPlacer({ place });

    manager.showWidget();

    expect(widgetWindow.showInactive).toHaveBeenCalledOnce();
    expect(widgetWindow.setBounds).not.toHaveBeenCalled();
    await vi.waitFor(() => expect(place).toHaveBeenCalledOnce());
    // Bottom-centre of the mocked 1920x1040 work area: (1920-640)/2 = 640,
    // 1040 - 320 - 24px dock clearance = 696.
    expect(place).toHaveBeenCalledWith({
      title: WIDGET_WINDOW_TITLE,
      x: 640,
      y: 696,
      above: true,
      sticky: true,
    });
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
    // 24px idle pill: (640, 500) over the 1920x1040 work area.
    expect(settingsService.updateSettings).toHaveBeenCalledWith({
      ui: {
        theme: "system",
        widgetPosition: { xRatio: 640 / 1920, yRatio: 500 / 1040 },
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
});
