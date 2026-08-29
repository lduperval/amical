import type { AppSettingsData } from "../schema";
import { isLinux } from "../../utils/platform";
import { WINDOWS_KEYCODES } from "../../utils/keycodes";
import { LINUX_DEFAULT_SHORTCUTS } from "../../utils/linux-default-shortcuts";

const OLD_WINDOWS_DEFAULTS = {
  pushToTalk: [WINDOWS_KEYCODES.CTRL, WINDOWS_KEYCODES.WIN],
  toggleRecording: [
    WINDOWS_KEYCODES.CTRL,
    WINDOWS_KEYCODES.WIN,
    WINDOWS_KEYCODES.SPACE,
  ],
  pasteLastTranscript: [
    WINDOWS_KEYCODES.ALT,
    WINDOWS_KEYCODES.SHIFT,
    WINDOWS_KEYCODES.Z,
  ],
  newNote: [WINDOWS_KEYCODES.ALT, WINDOWS_KEYCODES.SHIFT, WINDOWS_KEYCODES.N],
  draftMode: [
    WINDOWS_KEYCODES.CTRL,
    WINDOWS_KEYCODES.WIN,
    WINDOWS_KEYCODES.ALT,
  ],
} as const;

const sameChord = (left: readonly number[], right: readonly number[]) =>
  left.length === right.length &&
  [...left]
    .sort((a, b) => a - b)
    .every((key, index) => {
      return key === [...right].sort((a, b) => a - b)[index];
    });

// Early Linux builds seeded the Windows defaults even though Linux uses the
// macOS keycode table, and two of those defaults were modifier-only. Replace
// only those exact seeded chords; all user-defined bindings remain untouched.
export function migrateToV16(data: unknown): AppSettingsData {
  const oldData = data as AppSettingsData;
  if (!isLinux() || !oldData.shortcuts) return oldData;

  const migratedShortcuts = { ...oldData.shortcuts };
  for (const type of Object.keys(OLD_WINDOWS_DEFAULTS) as Array<
    keyof typeof OLD_WINDOWS_DEFAULTS
  >) {
    migratedShortcuts[type] = oldData.shortcuts[type]?.map((binding) =>
      sameChord(binding, OLD_WINDOWS_DEFAULTS[type])
        ? [...LINUX_DEFAULT_SHORTCUTS[type]]
        : binding,
    );
  }

  return { ...oldData, shortcuts: migratedShortcuts };
}
