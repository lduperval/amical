import { app } from "electron";
import started from "electron-squirrel-startup";
import { showFatalStartupDialog } from "./fatal-startup-dialog";
import { shouldUseXWaylandForFloatingWidget } from "../utils/linux-windowing";

// Configure Chromium synchronously in the entry point, before loading the
// application's dependency graph. These switches and the acceleration setting
// must be applied before Electron initializes its display/GPU services.
if (!started && process.platform === "linux") {
  if (
    shouldUseXWaylandForFloatingWidget({
      platform: process.platform,
      sessionType: process.env.XDG_SESSION_TYPE,
      argv: process.argv,
    })
  ) {
    app.commandLine.appendSwitch("ozone-platform", "x11");
  }

  if (!process.argv.includes("--enable-gpu")) {
    app.disableHardwareAcceleration();
    // Explicitly keep window compositing on the software path as well. This
    // avoids GPU-backed presentation for the main window alongside the
    // transparent floating widget on Linux/XWayland.
    app.commandLine.appendSwitch("disable-gpu-compositing");
  }
}

// E2E harness hook (see e2e/): give each test run an isolated profile. Must
// happen before ./app loads — requestSingleInstanceLock() there keys its lock
// off userData, and an isolated path keeps test instances from colliding with
// a real running Amical. sessionData is set too so Chromium caches follow.
if (process.env.AMICAL_E2E_USER_DATA_DIR) {
  app.setPath("userData", process.env.AMICAL_E2E_USER_DATA_DIR);
  app.setPath("sessionData", process.env.AMICAL_E2E_USER_DATA_DIR);
}

if (started) {
  // Squirrel.Windows event hook process (--squirrel-install/-updated/
  // -obsolete/-uninstall): electron-squirrel-startup spawns the Update.exe
  // shortcut work and quits once it completes. Nothing else may run here —
  // loading the app would reach requestSingleInstanceLock(), which fires
  // second-instance in the already-running app and pops the main window
  // mid-background-update.
  app.quit();
} else {
  // The entire app lives behind this dynamic import so a module-evaluation
  // failure anywhere in its graph (broken native binding, quarantined file)
  // rejects here — where the user can still be told — instead of crashing the
  // process before any error handling exists. Keep this entry's own imports
  // minimal for the same reason. The fatal-dialog helper retains showErrorBox
  // as its pre-ready-safe fallback.
  import("./app").catch(async (error: unknown) => {
    console.error("Failed to load application", error);
    await showFatalStartupDialog(error, "module_load");
    app.exit(1);
  });
}
