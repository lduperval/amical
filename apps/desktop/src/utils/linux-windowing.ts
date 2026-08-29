/**
 * Native Wayland does not let Electron position windows, keep them always on
 * top, or show them without focus. Amical's floating widget needs all three,
 * so Wayland sessions use XWayland unless the user explicitly selected an
 * Ozone platform on the command line.
 */
export function shouldUseXWaylandForFloatingWidget({
  platform,
  sessionType,
  argv,
}: {
  platform: NodeJS.Platform;
  sessionType: string | undefined;
  argv: string[];
}): boolean {
  return (
    platform === "linux" &&
    sessionType?.toLowerCase() === "wayland" &&
    !argv.some(
      (argument) =>
        argument === "--ozone-platform" ||
        argument.startsWith("--ozone-platform="),
    )
  );
}
