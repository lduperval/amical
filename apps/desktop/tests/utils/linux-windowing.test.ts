import { describe, expect, it } from "vitest";
import { shouldUseXWaylandForFloatingWidget } from "../../src/utils/linux-windowing";

describe("Linux floating-widget windowing mode", () => {
  it("uses XWayland for a Wayland session", () => {
    expect(
      shouldUseXWaylandForFloatingWidget({
        platform: "linux",
        sessionType: "wayland",
        argv: ["amical"],
      }),
    ).toBe(true);
  });

  it("does not override an explicit Ozone platform", () => {
    expect(
      shouldUseXWaylandForFloatingWidget({
        platform: "linux",
        sessionType: "wayland",
        argv: ["amical", "--ozone-platform=wayland"],
      }),
    ).toBe(false);
  });

  it("does not alter X11 or non-Linux sessions", () => {
    expect(
      shouldUseXWaylandForFloatingWidget({
        platform: "linux",
        sessionType: "x11",
        argv: ["amical"],
      }),
    ).toBe(false);
    expect(
      shouldUseXWaylandForFloatingWidget({
        platform: "darwin",
        sessionType: undefined,
        argv: ["amical"],
      }),
    ).toBe(false);
  });
});
