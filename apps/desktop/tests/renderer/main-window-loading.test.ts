// @vitest-environment jsdom

import React from "react";
import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

const gate = vi.hoisted(() => {
  let resolve!: () => void;
  const promise = new Promise<void>((done) => {
    resolve = done;
  });
  return { promise, resolve: () => resolve() };
});

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: () => "Loading Amical…" }),
}));

vi.mock("../../src/renderer/main/routeTree.gen", async () => {
  const { createRootRoute, createRoute } = await import(
    "@tanstack/react-router"
  );
  const root = createRootRoute();
  const index = createRoute({
    getParentRoute: () => root,
    path: "/",
    loader: () => gate.promise,
    component: () => "Main window ready",
  });
  return { routeTree: root.addChildren([index]) };
});

import MainContent from "../../src/renderer/main/content";

afterEach(cleanup);

it("keeps a visible loading state while the main route is pending", async () => {
  render(React.createElement(MainContent));
  expect((await screen.findByRole("status")).textContent).toContain(
    "Loading Amical…",
  );
  expect(screen.queryByText("Main window ready")).toBeNull();

  await act(async () => gate.resolve());
  expect(await screen.findByText("Main window ready")).toBeTruthy();
});
