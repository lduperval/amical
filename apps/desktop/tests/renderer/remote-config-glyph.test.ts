// @vitest-environment jsdom

import React from "react";
import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

const loaders = vi.hoisted(() => ({
  cloud: vi.fn(),
  info: vi.fn(),
  broken: vi.fn(),
}));
vi.mock("lucide-react/dynamicIconImports", () => ({ default: loaders }));

import { RemoteConfigGlyph } from "../../src/renderer/main/components/remote-config-glyph";

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

it("loads only the requested icon without suspending the surrounding layout", async () => {
  let resolve!: (value: { default: () => React.ReactNode }) => void;
  loaders.cloud.mockReturnValue(
    new Promise((done) => {
      resolve = done;
    }),
  );
  expect(loaders.cloud).not.toHaveBeenCalled();
  render(
    React.createElement(
      "div",
      null,
      "Main interface",
      React.createElement(RemoteConfigGlyph, {
        name: "cloud",
        fallbackName: "info",
      }),
    ),
  );
  expect(screen.getByText("Main interface")).toBeTruthy();
  expect(loaders.cloud).toHaveBeenCalledOnce();
  expect(loaders.info).not.toHaveBeenCalled();
  expect(loaders.broken).not.toHaveBeenCalled();

  await act(async () =>
    resolve({
      default: () =>
        React.createElement("svg", { "data-testid": "cloud-glyph" }),
    }),
  );
  expect(await screen.findByTestId("cloud-glyph")).toBeTruthy();
});

it("uses the tone's fallback for an unknown name", async () => {
  loaders.info.mockResolvedValue({
    default: () => React.createElement("svg", { "data-testid": "info-glyph" }),
  });
  render(
    React.createElement(RemoteConfigGlyph, {
      name: "unknown",
      fallbackName: "info",
    }),
  );
  expect(await screen.findByTestId("info-glyph")).toBeTruthy();
});

it("keeps the layout usable if an icon chunk fails to load", async () => {
  const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
  loaders.broken.mockRejectedValue(new Error("chunk unavailable"));
  try {
    render(
      React.createElement(
        "div",
        null,
        "Main interface",
        React.createElement(RemoteConfigGlyph, {
          name: "broken",
          fallbackName: "info",
        }),
      ),
    );
    await act(async () => {});
    expect(screen.getByText("Main interface")).toBeTruthy();
    expect(warn).toHaveBeenCalledWith(
      "Failed to load remote-config icon",
      expect.objectContaining({ name: "broken" }),
    );
  } finally {
    warn.mockRestore();
  }
});
