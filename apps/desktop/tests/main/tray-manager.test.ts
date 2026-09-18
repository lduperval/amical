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
import { TrayManager } from "@main/managers/tray-manager";
import type { WindowManager } from "@main/core/window-manager";

const originalPlatform = process.platform;
const setPlatform = (value: NodeJS.Platform) =>
  Object.defineProperty(process, "platform", { configurable: true, value });

afterEach(() => {
  vi.restoreAllMocks();
});

afterAll(() => {
  setPlatform(originalPlatform);
});

const createWindowManager = (onboardingOpen = false) => {
  const onboardingWindow = onboardingOpen
    ? { isDestroyed: vi.fn(() => false), show: vi.fn(), focus: vi.fn() }
    : null;
  return {
    windowManager: {
      getOnboardingWindow: vi.fn(() => onboardingWindow),
      createOrShowMainWindow: vi.fn(async () => undefined),
    } as unknown as WindowManager,
    onboardingWindow,
  };
};

const trayOf = (manager: TrayManager): EventEmitter =>
  Reflect.get(manager, "tray") as EventEmitter;

describe("TrayManager icon activation", () => {
  beforeEach(() => {
    // The singleton keeps its Tray between tests; start each one fresh.
    Reflect.set(TrayManager, "instance", null);
  });

  it("opens the console on tray activation (double-click) on Linux", async () => {
    setPlatform("linux");
    const { windowManager } = createWindowManager();
    const manager = TrayManager.getInstance();
    await manager.initialize(windowManager, "en");

    const tray = trayOf(manager);
    expect(tray.listenerCount("click")).toBe(1);
    expect(tray.listenerCount("double-click")).toBe(0);

    tray.emit("click");
    await vi.waitFor(() =>
      expect(windowManager.createOrShowMainWindow).toHaveBeenCalledOnce(),
    );
  });

  it("opens the console on double-click elsewhere and leaves single clicks to the menu", async () => {
    setPlatform("win32");
    const { windowManager } = createWindowManager();
    const manager = TrayManager.getInstance();
    await manager.initialize(windowManager, "en");

    const tray = trayOf(manager);
    expect(tray.listenerCount("click")).toBe(0);
    tray.emit("click");
    expect(windowManager.createOrShowMainWindow).not.toHaveBeenCalled();

    tray.emit("double-click");
    await vi.waitFor(() =>
      expect(windowManager.createOrShowMainWindow).toHaveBeenCalledOnce(),
    );
  });

  it("focuses onboarding instead of opening the main window while the wizard is open", async () => {
    setPlatform("linux");
    const { windowManager, onboardingWindow } = createWindowManager(true);
    const manager = TrayManager.getInstance();
    await manager.initialize(windowManager, "en");

    await manager.openConsole("test");

    expect(onboardingWindow?.show).toHaveBeenCalledOnce();
    expect(onboardingWindow?.focus).toHaveBeenCalledOnce();
    expect(windowManager.createOrShowMainWindow).not.toHaveBeenCalled();
  });
});
