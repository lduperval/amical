import { describe, expect, it } from "vitest";
import {
  getLinuxOzonePlatform,
  getLinuxWindowingMode,
} from "../../src/utils/linux-windowing";

describe("Linux floating-widget windowing mode", () => {
  it("uses native Wayland with Electron 44", () => {
    expect(
      getLinuxOzonePlatform({
        platform: "linux",
        sessionType: "wayland",
        argv: ["amical"],
        electronVersion: "44.3.0",
      }),
    ).toBe("wayland");
  });

  it("uses XWayland with the earlier working Electron version", () => {
    expect(
      getLinuxOzonePlatform({
        platform: "linux",
        sessionType: "wayland",
        argv: ["amical"],
        electronVersion: "38.1.0",
      }),
    ).toBe("x11");
  });

  it("does not override an explicit Ozone platform", () => {
    expect(
      getLinuxOzonePlatform({
        platform: "linux",
        sessionType: "wayland",
        argv: ["amical", "--ozone-platform=wayland"],
        electronVersion: "44.3.0",
      }),
    ).toBeUndefined();
  });

  it("does not alter X11 or non-Linux sessions", () => {
    expect(
      getLinuxOzonePlatform({
        platform: "linux",
        sessionType: "x11",
        argv: ["amical"],
        electronVersion: "44.3.0",
      }),
    ).toBeUndefined();
    expect(
      getLinuxOzonePlatform({
        platform: "darwin",
        sessionType: undefined,
        argv: ["amical"],
        electronVersion: "44.3.0",
      }),
    ).toBeUndefined();
  });
});

describe("Linux windowing mode", () => {
  it("reports native Wayland only when the Ozone switch says so", () => {
    expect(
      getLinuxWindowingMode({ platform: "linux", ozonePlatform: "wayland" }),
    ).toBe("wayland");
    expect(
      getLinuxWindowingMode({ platform: "linux", ozonePlatform: "x11" }),
    ).toBe("x11");
    // Electron's Linux default without the switch is X11/XWayland.
    expect(
      getLinuxWindowingMode({ platform: "linux", ozonePlatform: undefined }),
    ).toBe("x11");
    expect(
      getLinuxWindowingMode({ platform: "darwin", ozonePlatform: "wayland" }),
    ).toBe("none");
  });
});
