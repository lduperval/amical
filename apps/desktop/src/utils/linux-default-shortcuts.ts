import { MAC_KEYCODES } from "./keycodes";

// Linux uses the macOS virtual-keycode space in keycode-map.ts. Every Wayland
// portal shortcut also needs exactly one non-modifier trigger key; modifier-only
// defaults inherited from Windows cannot be represented by the portal.
export const LINUX_DEFAULT_SHORTCUTS = {
  pushToTalk: [MAC_KEYCODES.CTRL, MAC_KEYCODES.CMD, MAC_KEYCODES.SPACE],
  toggleRecording: [
    MAC_KEYCODES.CTRL,
    MAC_KEYCODES.CMD,
    MAC_KEYCODES.ALT,
    MAC_KEYCODES.SPACE,
  ],
  pasteLastTranscript: [MAC_KEYCODES.ALT, MAC_KEYCODES.SHIFT, MAC_KEYCODES.Z],
  newNote: [MAC_KEYCODES.ALT, MAC_KEYCODES.SHIFT, MAC_KEYCODES.N],
  draftMode: [
    MAC_KEYCODES.CTRL,
    MAC_KEYCODES.CMD,
    MAC_KEYCODES.ALT,
    MAC_KEYCODES.D,
  ],
} as const;
