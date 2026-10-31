// @vitest-environment jsdom

import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useWidgetOutline } from "@/renderer/widget/hooks/useWidgetOutline";

const subscription = vi.hoisted(() => ({ onData: (_enabled: boolean) => {} }));
vi.mock("@/trpc/react", () => ({
  api: {
    settings: {
      widgetOutlinePreference: {
        useSubscription: (_input: undefined, options: typeof subscription) => {
          subscription.onData = options.onData;
        },
      },
    },
  },
}));

afterEach(() => {
  cleanup();
  document.body.innerHTML = "";
});

describe("widget outline", () => {
  it("starts transparent, follows live changes, and clears on unmount", () => {
    const root = document.createElement("div");
    root.id = "root";
    document.body.appendChild(root);
    const { unmount } = renderHook(() => useWidgetOutline());

    expect(root.classList.contains("show-widget-outline")).toBe(false);
    act(() => subscription.onData(true));
    expect(root.classList.contains("show-widget-outline")).toBe(true);
    act(() => subscription.onData(false));
    expect(root.classList.contains("show-widget-outline")).toBe(false);
    act(() => subscription.onData(true));
    unmount();
    expect(root.classList.contains("show-widget-outline")).toBe(false);
  });
});
