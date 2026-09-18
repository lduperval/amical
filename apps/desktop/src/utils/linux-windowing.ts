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
