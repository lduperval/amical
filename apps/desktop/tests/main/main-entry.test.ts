import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

let appModuleLoaded = false;
let accelerationDisabledAtAppImport = false;

// vi.mock factories are cached in the mock registry and survive
// vi.resetModules(), so per-test behavior (squirrel flag, app-module failure)
// must be registered with vi.doMock before each fresh import of the entry.
async function importEntry(opts: { started?: boolean; appError?: Error } = {}) {
  vi.resetModules();
  vi.doMock("electron-squirrel-startup", () => ({
    default: opts.started ?? false,
  }));
  vi.doMock("@/main/app", async () => {
    const { app } = await import("electron");
    accelerationDisabledAtAppImport =
      vi.mocked(app.disableHardwareAcceleration).mock.calls.length > 0;
    if (opts.appError) throw opts.appError;
    appModuleLoaded = true;
    return {};
  });

  await import("@/main/main");
  await vi.dynamicImportSettled();
  // dynamicImportSettled resolves when the import settles; the entry's .catch
  // continuation runs a tick later.
  await new Promise((resolve) => setTimeout(resolve, 0));

  // Return the electron mock instance the entry actually used.
  return await import("electron");
}

describe("main entry", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    appModuleLoaded = false;
    accelerationDisabledAtAppImport = false;
  });

  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllEnvs();
  });

  it("configures Linux software rendering before loading the app module", async () => {
    vi.spyOn(process, "platform", "get").mockReturnValue("linux");
    vi.spyOn(process, "argv", "get").mockReturnValue(["amical"]);
    vi.stubEnv("XDG_SESSION_TYPE", "wayland");
    const { app } = await importEntry();

    expect(accelerationDisabledAtAppImport).toBe(true);
    expect(app.disableHardwareAcceleration).toHaveBeenCalledOnce();
    expect(app.commandLine.appendSwitch).toHaveBeenCalledWith(
      "ozone-platform",
      "x11",
    );
    expect(app.commandLine.appendSwitch).toHaveBeenCalledWith(
      "disable-gpu-compositing",
    );
  });

  it("preserves explicit GPU and Ozone overrides", async () => {
    vi.spyOn(process, "platform", "get").mockReturnValue("linux");
    vi.spyOn(process, "argv", "get").mockReturnValue([
      "amical",
      "--enable-gpu",
      "--ozone-platform=wayland",
    ]);
    vi.stubEnv("XDG_SESSION_TYPE", "wayland");
    const { app } = await importEntry();

    expect(app.disableHardwareAcceleration).not.toHaveBeenCalled();
    expect(app.commandLine.appendSwitch).not.toHaveBeenCalled();
  });

  it("leaves rendering unchanged outside Linux", async () => {
    vi.spyOn(process, "platform", "get").mockReturnValue("darwin");
    const { app } = await importEntry();

    expect(app.disableHardwareAcceleration).not.toHaveBeenCalled();
    expect(app.commandLine.appendSwitch).not.toHaveBeenCalled();
  });

  it("loads the app module", async () => {
    const { app, dialog } = await importEntry();

    expect(appModuleLoaded).toBe(true);
    expect(dialog.showMessageBox).not.toHaveBeenCalled();
    expect(app.exit).not.toHaveBeenCalled();
  });

  it("shows an error dialog and exits when the app module fails to load", async () => {
    const { app, dialog } = await importEntry({
      appError: new Error("boom"),
    });

    expect(appModuleLoaded).toBe(false);
    expect(dialog.showMessageBox).toHaveBeenCalledWith(
      expect.objectContaining({
        type: "error",
        title: "Amical failed to start",
        detail: expect.stringContaining("Stage: module_load"),
      }),
    );
    expect(app.exit).toHaveBeenCalledWith(1);
  });

  it("quits when invoked as a Squirrel event hook", async () => {
    const { app } = await importEntry({ started: true });

    expect(app.quit).toHaveBeenCalled();
    // Hook processes must never load the app: reaching
    // requestSingleInstanceLock() would fire second-instance in the running
    // app and pop the main window mid-background-update.
    expect(appModuleLoaded).toBe(false);
  });
});
