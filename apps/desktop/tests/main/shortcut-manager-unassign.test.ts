import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ShortcutManager } from "../../src/main/managers/shortcut-manager";

describe.each(["darwin", "linux"])(
  "ShortcutManager shortcut unassignment on %s",
  (platform) => {
    const originalPlatform = Object.getOwnPropertyDescriptor(
      process,
      "platform",
    )!;
    const pttChord = platform === "linux" ? [59, 49] : [63];
    const draftChord = platform === "linux" ? [59, 2] : [63, 59];

    beforeEach(() => {
      Object.defineProperty(process, "platform", {
        configurable: true,
        value: platform,
      });
    });

    afterEach(() => {
      Object.defineProperty(process, "platform", originalPlatform);
    });
    it("rejects unassigning push-to-talk", async () => {
      const setShortcuts = vi.fn().mockResolvedValue(undefined);
      const syncNativeShortcuts = vi.fn().mockResolvedValue(true);
      const manager = ShortcutManager.createForTests(
        { setShortcuts } as never,
        { setShortcuts: syncNativeShortcuts } as never,
      );

      await expect(
        manager.setShortcutBindings("pushToTalk", []),
      ).resolves.toEqual({
        valid: false,
        error: { key: "settings.shortcuts.validation.noKeysDetected" },
      });
      expect(setShortcuts).not.toHaveBeenCalled();
      expect(syncNativeShortcuts).not.toHaveBeenCalled();
    });

    it("persists an empty shortcut and removes it from native matching", async () => {
      const setShortcuts = vi.fn().mockResolvedValue(undefined);
      const syncNativeShortcuts = vi.fn().mockResolvedValue(true);
      const manager = ShortcutManager.createForTests(
        { setShortcuts } as never,
        { setShortcuts: syncNativeShortcuts } as never,
      );

      await manager.setShortcutBindings("pushToTalk", [pttChord]);
      await manager.setShortcutBindings("newNote", [[55, 59, 45]]);
      setShortcuts.mockClear();
      syncNativeShortcuts.mockClear();

      await expect(manager.setShortcutBindings("newNote", [])).resolves.toEqual(
        {
          valid: true,
        },
      );
      expect(setShortcuts).toHaveBeenCalledWith({
        pushToTalk: [pttChord],
        toggleRecording: [],
        pasteLastTranscript: [],
        newNote: [],
        draftMode: [],
      });
      expect(syncNativeShortcuts).toHaveBeenCalledWith({
        subsetChords: [pttChord],
        exactChords: [],
      });
    });

    it("serializes persistence and native synchronization", async () => {
      const setShortcuts = vi.fn().mockResolvedValue(undefined);
      const syncNativeShortcuts = vi.fn().mockResolvedValue(true);
      const manager = ShortcutManager.createForTests(
        { setShortcuts } as never,
        { setShortcuts: syncNativeShortcuts } as never,
      );

      await manager.setShortcutBindings("newNote", [[55, 59, 45]]);
      setShortcuts.mockClear();
      syncNativeShortcuts.mockClear();

      let releaseFirstNativeSync!: () => void;
      const firstNativeSync = new Promise<boolean>((resolve) => {
        releaseFirstNativeSync = () => resolve(true);
      });
      syncNativeShortcuts.mockImplementationOnce(() => firstNativeSync);

      const clearNewNote = manager.setShortcutBindings("newNote", []);
      const updateDraftMode = manager.setShortcutBindings("draftMode", [
        draftChord,
      ]);

      await vi.waitFor(() => {
        expect(syncNativeShortcuts).toHaveBeenCalledTimes(1);
      });
      expect(setShortcuts).toHaveBeenCalledTimes(1);

      releaseFirstNativeSync();
      await Promise.all([clearNewNote, updateDraftMode]);

      expect(setShortcuts).toHaveBeenNthCalledWith(1, {
        pushToTalk: [],
        toggleRecording: [],
        pasteLastTranscript: [],
        newNote: [],
        draftMode: [],
      });
      expect(setShortcuts).toHaveBeenNthCalledWith(2, {
        pushToTalk: [],
        toggleRecording: [],
        pasteLastTranscript: [],
        newNote: [],
        draftMode: [draftChord],
      });
      expect(syncNativeShortcuts).toHaveBeenNthCalledWith(1, {
        subsetChords: [],
        exactChords: [],
      });
      expect(syncNativeShortcuts).toHaveBeenNthCalledWith(2, {
        subsetChords: [draftChord],
        exactChords: [],
      });
    });

    it("validates a queued mutation against the preceding result", async () => {
      const setShortcuts = vi.fn().mockResolvedValue(undefined);
      const syncNativeShortcuts = vi.fn().mockResolvedValue(true);
      const manager = ShortcutManager.createForTests(
        { setShortcuts } as never,
        { setShortcuts: syncNativeShortcuts } as never,
      );
      const shortcut = [55, 59, 45];

      const assignNewNote = manager.setShortcutBindings("newNote", [shortcut]);
      const assignDraftMode = manager.setShortcutBindings("draftMode", [
        shortcut,
      ]);

      await expect(assignNewNote).resolves.toEqual({ valid: true });
      await expect(assignDraftMode).resolves.toEqual({
        valid: false,
        error: { key: "settings.shortcuts.validation.alreadyAssigned" },
      });
      expect(setShortcuts).toHaveBeenCalledOnce();
      expect(syncNativeShortcuts).toHaveBeenCalledOnce();
    });

    it("continues processing after a failed mutation", async () => {
      const setShortcuts = vi
        .fn()
        .mockRejectedValueOnce(new Error("write failed"))
        .mockResolvedValue(undefined);
      const syncNativeShortcuts = vi.fn().mockResolvedValue(true);
      const manager = ShortcutManager.createForTests(
        { setShortcuts } as never,
        { setShortcuts: syncNativeShortcuts } as never,
      );

      const failedMutation = manager.setShortcutBindings("newNote", [
        [55, 59, 45],
      ]);
      const failedExpectation =
        expect(failedMutation).rejects.toThrow("write failed");
      const nextMutation = manager.setShortcutBindings("draftMode", [
        draftChord,
      ]);

      await failedExpectation;
      await expect(nextMutation).resolves.toEqual({ valid: true });
      expect(setShortcuts).toHaveBeenCalledTimes(2);
      expect(setShortcuts).toHaveBeenNthCalledWith(2, {
        pushToTalk: [],
        toggleRecording: [],
        pasteLastTranscript: [],
        newNote: [],
        draftMode: [draftChord],
      });
      expect(syncNativeShortcuts).toHaveBeenCalledOnce();
      expect(syncNativeShortcuts).toHaveBeenCalledWith({
        subsetChords: [draftChord],
        exactChords: [],
      });
    });
  },
);
