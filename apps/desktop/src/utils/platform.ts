/**
 * Platform detection utilities
 */

export type Platform = NodeJS.Platform;

function getRendererPlatform(): Platform | undefined {
  if (typeof window === "undefined") return undefined;
  return window.electronAPI?.platform;
}

function getNodePlatform(): Platform | undefined {
  if (typeof process === "undefined") return undefined;
  return process.platform;
}

export function getPlatform(): Platform {
  return getRendererPlatform() ?? getNodePlatform() ?? ("unknown" as Platform);
}

export function isWindows(): boolean {
  return getPlatform() === "win32";
}

export function isMacOS(): boolean {
  return getPlatform() === "darwin";
}

export function isLinux(): boolean {
  return getPlatform() === "linux";
}

/**
 * Get the native helper name for the current platform
 */
export function getNativeHelperName(): string {
  if (isWindows()) return "WindowsHelper.exe";
  if (isLinux()) return "linux-helper";
  return "SwiftHelper";
}

/**
 * Get the native helper directory name for the current platform
 */
export function getNativeHelperDir(): string {
  if (isWindows()) return "windows-helper";
  if (isLinux()) return "linux-helper";
  return "swift-helper";
}

/**
 * Get a platform-specific display name
 */
export function getPlatformDisplayName(): string {
  const platform = getPlatform();
  switch (platform) {
    case "darwin":
      return "macOS";
    case "win32":
      return "Windows";
    case "linux":
      return "Linux";
    default:
      return platform;
  }
}
