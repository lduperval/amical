# Pause media during dictation (Windows)

With **Mute system audio during dictation** enabled, the Windows helper also
pauses media sessions that Windows reports as playing and controllable. At the
end of recording (including cancellation), it resumes only sessions it paused.
The existing volume mute remains as a fallback for unsupported players and
other sounds. Disabling the existing preference disables both behaviors.

This uses Windows' media session API, not a simulated Play/Pause key. A browser
must expose the tab through Windows media controls; YouTube normally does.
Videos are treated like music. Live streams may return to the live position
instead of preserving everything broadcast while paused.

Players already paused are left alone. Playback changes, track changes, closed
players, and unavailable controls suppress automatic resume. Windows only
reports changes exposed by the player: a redundant Pause command on an already
paused player may be invisible. A forced exit of Amical/helper cannot guarantee
resume; use the player's Play control if necessary.

## Build and check

From the repository root, with Node >=24, pnpm 10.34.5 and .NET 8+ installed:

```powershell
pnpm install --frozen-lockfile
pnpm --filter @amical/types generate:all
dotnet test packages/native-helpers/windows-helper-tests/WindowsHelper.Tests.csproj -c Release
pnpm --filter @amical/windows-helper build:native
```

Restart the development app after rebuilding the helper; an installed Amical
uses its own bundled helper and does not pick up this source change.

Development builds identify themselves as `AmicalDev` and use a separate
application identity. Restart the development app after source changes.
Keep personal launchers, profiles and database backups outside tracked files.

## Manual browser checks

1. Enable the existing system-audio mute preference in the test build.
2. Play a YouTube video, start dictation, wait 10 seconds, then stop. The video
   should pause, its position should stay still, then it should resume there.
3. Repeat with the video already paused. It must stay paused afterward.
4. Repeat, cancelling dictation instead of submitting it. Playback must resume.
5. While dictating, switch tracks or play/pause manually. Amical should not
   restart the changed session. Close the tab during dictation: no error should
   interrupt the next dictation.
6. Dictate twice quickly, then try with the mute preference disabled.

## Keeping the fork easy to update

The media pause implementation and tests live in the Windows helper directories.
They require no shared TypeScript schema, database, translations, Linux, or macOS edits.
The only integration points are optional callbacks in RecordingAmbianceController
and their wiring in RpcHandler. Both .csproj files target Windows SDK 19041 to
access WinRT from .NET 8; the package lock is unchanged.

Development identity is a separate small change in `main.ts`, `app.ts`, and
`tray-manager.ts`.

Keep this feature as a separate commit. Development and testing may use
`codex/windows-dictation-media-pause`, but integrate tested changes into `main`
and push only `main` to `origin`, per this fork's workflow.
That allows rebasing or cherry-picking the feature onto future fork updates
without carrying an unrelated refactor. Merge conflicts remain possible if
upstream changes these integration points.

API reference: https://learn.microsoft.com/en-us/uwp/api/windows.media.control.globalsystemmediatransportcontrolssession
