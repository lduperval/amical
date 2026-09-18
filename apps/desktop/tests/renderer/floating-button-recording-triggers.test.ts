// @vitest-environment jsdom

import React from "react";
import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { RecordingStatus } from "@/hooks/useRecording";

const mocks = vi.hoisted(() => ({
  dragWidget: vi.fn(),
  linuxWindowingMode: undefined as "x11" | "wayland" | "none" | undefined,
}));

vi.mock("@/components/Waveform", () => ({
  Waveform: () => null,
}));

vi.mock("@/hooks/useFeatureFlag", () => ({
  useFeatureFlag: () => ({ enabled: false }),
}));

vi.mock("@/trpc/react", () => ({
  api: {
    widget: {
      openNotesWindow: {
        useMutation: () => ({ mutateAsync: vi.fn() }),
      },
      drag: {
        useMutation: () => ({ mutate: mocks.dragWidget }),
      },
      linuxWindowingMode: {
        useQuery: () => ({ data: mocks.linuxWindowingMode }),
      },
    },
  },
}));

vi.mock("@/renderer/widget/pass-through", () => ({
  setPassThroughReason: vi.fn(),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

import { FloatingButton } from "@/renderer/widget/pages/widget/components/FloatingButton";

afterEach(() => {
  vi.clearAllMocks();
  mocks.linuxWindowingMode = undefined;
  Object.defineProperty(window, "electronAPI", {
    configurable: true,
    value: undefined,
  });
});

describe("FloatingButton recording triggers", () => {
  it("keeps an idle Linux recording control visible and clickable", () => {
    Object.defineProperty(window, "electronAPI", {
      configurable: true,
      value: { platform: "linux" },
    });
    const startRecording = vi.fn().mockResolvedValue(undefined);

    render(
      React.createElement(FloatingButton, {
        recordingStatus: {
          sessionId: null,
          state: "idle",
          mode: "ptt",
          isDraft: false,
          stopKind: "none",
          stopOrigin: "none",
        },
        audioLevels: [],
        startRecording,
        stopRecording: vi.fn(),
        dismissRecording: vi.fn(),
      }),
    );

    fireEvent.click(screen.getByRole("button", { name: "Start recording" }));
    expect(startRecording).toHaveBeenCalledOnce();
  });

  it("moves the bubble with a middle-button pointer drag", () => {
    Object.defineProperty(window, "electronAPI", {
      configurable: true,
      value: { platform: "linux" },
    });

    render(
      React.createElement(FloatingButton, {
        recordingStatus: {
          sessionId: null,
          state: "idle",
          mode: "ptt",
          isDraft: false,
          stopKind: "none",
          stopOrigin: "none",
        },
        audioLevels: [],
        startRecording: vi.fn(),
        stopRecording: vi.fn(),
        dismissRecording: vi.fn(),
      }),
    );

    const button = screen.getByRole("button", { name: "Start recording" });
    const bubble = button.parentElement?.parentElement;
    expect(bubble).not.toBeNull();

    fireEvent.pointerDown(bubble!, {
      button: 1,
      buttons: 4,
      pointerId: 7,
      screenX: 100,
      screenY: 200,
    });
    fireEvent.pointerMove(bubble!, {
      buttons: 4,
      pointerId: 7,
      screenX: 140,
      screenY: 230,
    });
    fireEvent.pointerUp(bubble!, {
      button: 1,
      pointerId: 7,
      screenX: 140,
      screenY: 230,
    });

    expect(mocks.dragWidget.mock.calls.map(([input]) => input)).toEqual([
      { phase: "start", screenX: 100, screenY: 200 },
      { phase: "move", screenX: 140, screenY: 230 },
      { phase: "end", screenX: 140, screenY: 230 },
    ]);
  });

  it("offers a compositor drag grip on native Wayland instead of main-process drags", () => {
    Object.defineProperty(window, "electronAPI", {
      configurable: true,
      value: { platform: "linux" },
    });
    mocks.linuxWindowingMode = "wayland";

    render(
      React.createElement(FloatingButton, {
        recordingStatus: {
          sessionId: null,
          state: "idle",
          mode: "ptt",
          isDraft: false,
          stopKind: "none",
          stopOrigin: "none",
        },
        audioLevels: [],
        startRecording: vi.fn(),
        stopRecording: vi.fn(),
        dismissRecording: vi.fn(),
      }),
    );

    // The grip hands the drag to the compositor (xdg_toplevel move); the
    // main process cannot move a native Wayland window itself.
    // jsdom does not model the vendor property, so read what React set.
    const grip = screen.getByTitle("Drag to move");
    expect(
      (grip.style as unknown as { WebkitAppRegion?: string }).WebkitAppRegion,
    ).toBe("drag");

    const button = screen.getByRole("button", { name: "Start recording" });
    const bubble = button.parentElement?.parentElement;
    fireEvent.pointerDown(bubble!, {
      button: 1,
      buttons: 4,
      pointerId: 3,
      screenX: 10,
      screenY: 10,
    });
    fireEvent.pointerUp(bubble!, {
      button: 1,
      pointerId: 3,
      screenX: 10,
      screenY: 10,
    });
    expect(mocks.dragWidget).not.toHaveBeenCalled();
  });

  it.each(
    [
      {
        state: "recording",
        button: "Dismiss recording",
        expected: "dismiss",
      },
      {
        state: "starting",
        button: "Dismiss recording",
        expected: "dismiss",
      },
      {
        state: "recording",
        button: "Stop recording and transcribe",
        expected: "stop",
      },
      {
        state: "starting",
        button: "Stop recording and transcribe",
        expected: "stop",
      },
    ].flatMap((test) =>
      (["ptt", "hands-free"] as const).map((mode) => ({ ...test, mode })),
    ),
  )(
    "routes $button while $state in $mode mode",
    ({ state, button, expected, mode }) => {
      const startRecording = vi.fn().mockResolvedValue(undefined);
      const stopRecording = vi.fn().mockResolvedValue(undefined);
      const dismissRecording = vi.fn().mockResolvedValue(undefined);
      const recordingStatus: RecordingStatus = {
        sessionId: "session-1",
        state: state as "starting" | "recording",
        mode,
        isDraft: false,
        stopKind: "none",
        stopOrigin: "none",
      };

      render(
        React.createElement(FloatingButton, {
          recordingStatus,
          audioLevels: [],
          startRecording,
          stopRecording,
          dismissRecording,
        }),
      );

      fireEvent.click(screen.getByRole("button", { name: button }));

      expect(stopRecording).toHaveBeenCalledTimes(expected === "stop" ? 1 : 0);
      expect(dismissRecording).toHaveBeenCalledTimes(
        expected === "dismiss" ? 1 : 0,
      );
      expect(startRecording).not.toHaveBeenCalled();
    },
  );
});
