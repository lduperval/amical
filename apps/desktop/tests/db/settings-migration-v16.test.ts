import { afterAll, beforeAll, describe, expect, it } from "vitest";
import {
  CURRENT_SETTINGS_VERSION,
  migrateSettings,
} from "../../src/db/settings-migrations";
import { migrateToV16 } from "../../src/db/settings-migrations/v16";
import { LINUX_DEFAULT_SHORTCUTS } from "../../src/utils/linux-default-shortcuts";
import { WINDOWS_KEYCODES } from "../../src/utils/keycodes";

const originalPlatform = process.platform;

beforeAll(() => {
  Object.defineProperty(process, "platform", {
    configurable: true,
    value: "linux",
  });
});

afterAll(() => {
  Object.defineProperty(process, "platform", {
    configurable: true,
    value: originalPlatform,
  });
});

describe("migrateToV16", () => {
  it("replaces Windows defaults accidentally seeded on Linux", () => {
    const result = migrateToV16({
      shortcuts: {
        pushToTalk: [[WINDOWS_KEYCODES.CTRL, WINDOWS_KEYCODES.WIN]],
        toggleRecording: [
          [WINDOWS_KEYCODES.CTRL, WINDOWS_KEYCODES.WIN, WINDOWS_KEYCODES.SPACE],
        ],
        pasteLastTranscript: [
          [WINDOWS_KEYCODES.ALT, WINDOWS_KEYCODES.SHIFT, WINDOWS_KEYCODES.Z],
        ],
        newNote: [
          [WINDOWS_KEYCODES.ALT, WINDOWS_KEYCODES.SHIFT, WINDOWS_KEYCODES.N],
        ],
        draftMode: [
          [WINDOWS_KEYCODES.CTRL, WINDOWS_KEYCODES.WIN, WINDOWS_KEYCODES.ALT],
        ],
      },
    });

    expect(result.shortcuts).toEqual(
      Object.fromEntries(
        Object.entries(LINUX_DEFAULT_SHORTCUTS).map(([type, chord]) => [
          type,
          [[...chord]],
        ]),
      ),
    );
  });

  it("preserves user-defined Linux bindings", () => {
    const shortcuts = {
      pushToTalk: [[59, 55, 58]],
      toggleRecording: [[59, 58, 49]],
    };

    expect(migrateToV16({ shortcuts }).shortcuts).toEqual(shortcuts);
  });

  it("is registered as the current migration", () => {
    expect(CURRENT_SETTINGS_VERSION).toBe(16);
    expect(
      migrateSettings(
        {
          shortcuts: {
            pushToTalk: [[WINDOWS_KEYCODES.CTRL, WINDOWS_KEYCODES.WIN]],
          },
        },
        15,
      ).shortcuts?.pushToTalk,
    ).toEqual([[...LINUX_DEFAULT_SHORTCUTS.pushToTalk]]);
  });
});
