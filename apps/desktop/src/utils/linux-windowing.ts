/** Choose the Linux display backend before Electron initializes Chromium. */
export function getLinuxOzonePlatform({
  platform,
  sessionType,
  argv,
  electronVersion,
}: {
  platform: NodeJS.Platform;
  sessionType: string | undefined;
  argv: string[];
  electronVersion: string;
}): "x11" | "wayland" | undefined {
  if (
    platform !== "linux" ||
    sessionType?.toLowerCase() !== "wayland" ||
    argv.some(
      (argument) =>
        argument === "--ozone-platform" ||
        argument.startsWith("--ozone-platform="),
    )
  ) {
    return undefined;
  }

  // Electron 44's GPU process crashes repeatedly on this GNOME/XWayland
  // session; its native Wayland backend starts and renders. Earlier Electron
  // versions use XWayland for reliable floating-widget positioning.
  const major = Number.parseInt(electronVersion, 10);
  return major >= 44 ? "wayland" : "x11";
}

/**
 * Which display backend Electron actually runs on. `ozonePlatform` is the
 * effective `--ozone-platform` switch (set by main.ts or by the user); when it
 * is absent Electron defaults to X11/XWayland on Linux.
 */
export type LinuxWindowingMode = "x11" | "wayland" | "none";

export function getLinuxWindowingMode({
  platform,
  ozonePlatform,
}: {
  platform: NodeJS.Platform;
  ozonePlatform: string | undefined;
}): LinuxWindowingMode {
  if (platform !== "linux") {
    return "none";
  }
  return ozonePlatform?.toLowerCase() === "wayland" ? "wayland" : "x11";
}
