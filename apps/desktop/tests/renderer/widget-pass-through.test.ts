import { beforeEach, describe, expect, it, vi } from "vitest";

const { mutate } = vi.hoisted(() => ({
  mutate: vi.fn(async (_input: unknown) => undefined),
}));
vi.mock("@/trpc/react", () => ({
  trpcClient: { widget: { setIgnoreMouseEvents: { mutate } } },
}));

beforeEach(() => {
  vi.resetModules();
  mutate.mockClear();
});

describe("widget mouse input reasons", () => {
  it("does not expand the input area while hovering or dragging the pill", async () => {
    const { setPassThroughReason } = await import(
      "@/renderer/widget/pass-through"
    );
    setPassThroughReason("hover", true);
    setPassThroughReason("drag", true);
    setPassThroughReason("hover", false);
    expect(mutate).toHaveBeenLastCalledWith({ ignore: false, expanded: false });
    setPassThroughReason("drag", false);
    expect(mutate).toHaveBeenLastCalledWith({ ignore: true, expanded: false });
  });

  it.each(["toast", "draft", "error"] as const)(
    "expands for %s and shrinks even if hover remains active",
    async (reason) => {
      const { setPassThroughReason } = await import(
        "@/renderer/widget/pass-through"
      );
      setPassThroughReason("hover", true);
      setPassThroughReason(reason, true);
      expect(mutate).toHaveBeenLastCalledWith({
        ignore: false,
        expanded: true,
      });
      setPassThroughReason(reason, false);
      expect(mutate).toHaveBeenLastCalledWith({
        ignore: false,
        expanded: false,
      });
    },
  );
});
