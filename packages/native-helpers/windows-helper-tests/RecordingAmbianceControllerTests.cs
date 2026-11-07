using NUnit.Framework;
using System;
using System.Collections.Concurrent;
using System.Threading;
using System.Threading.Tasks;
using WindowsHelper.Services;

namespace WindowsHelper.Tests;

[TestFixture]
public class RecordingAmbianceControllerTests
{
    [Test]
    public async Task StartRecordingAsync_WaitsForPlaybackBeforeMuting()
    {
        var events = new ConcurrentQueue<string>();
        var playbackStarted = NewCompletionSource();
        var releasePlayback = NewCompletionSource();
        var controller = new RecordingAmbianceController(
            async soundName =>
            {
                events.Enqueue($"play:{soundName}");
                playbackStarted.TrySetResult(true);
                await releasePlayback.Task;
            },
            () =>
            {
                events.Enqueue("mute");
                return true;
            },
            () => true,
            _ => { }
        );

        var operation = controller.StartRecordingAsync(shouldMute: true, muteSounds: false);
        await playbackStarted.Task;

        Assert.That(events, Is.EqualTo(new[] { "play:rec-start" }));
        releasePlayback.TrySetResult(true);
        Assert.That(await operation, Is.True);
        Assert.That(events, Is.EqualTo(new[] { "play:rec-start", "mute" }));
    }

    [Test]
    public async Task StartRecordingAsync_OnPlaybackFailure_LogsAndStillMutes()
    {
        var events = new ConcurrentQueue<string>();
        var controller = new RecordingAmbianceController(
            soundName =>
            {
                events.Enqueue($"play:{soundName}");
                throw new InvalidOperationException("BadDeviceId calling waveOutOpen");
            },
            () =>
            {
                events.Enqueue("mute");
                return true;
            },
            () => true,
            _ => events.Enqueue("log")
        );

        var success = await controller.StartRecordingAsync(
            shouldMute: true,
            muteSounds: false
        );

        Assert.That(success, Is.True);
        Assert.That(events, Is.EqualTo(new[] { "play:rec-start", "log", "mute" }));
    }

    [Test]
    public async Task StartAndStopOperations_DoNotOverlap()
    {
        var events = new ConcurrentQueue<string>();
        var firstPlaybackStarted = NewCompletionSource();
        var releaseFirstPlayback = NewCompletionSource();
        var controller = new RecordingAmbianceController(
            async soundName =>
            {
                events.Enqueue($"play:{soundName}");
                if (soundName == "rec-start")
                {
                    firstPlaybackStarted.TrySetResult(true);
                    await releaseFirstPlayback.Task;
                }
            },
            () =>
            {
                events.Enqueue("mute");
                return true;
            },
            () =>
            {
                events.Enqueue("restore");
                return true;
            },
            _ => { }
        );

        var start = controller.StartRecordingAsync(shouldMute: true, muteSounds: false);
        await firstPlaybackStarted.Task;
        var stop = controller.StopRecordingAsync(wasMuted: true, muteSounds: false);

        await Task.Delay(20);
        Assert.That(events, Is.EqualTo(new[] { "play:rec-start" }));

        releaseFirstPlayback.TrySetResult(true);
        Assert.That(await start, Is.True);
        Assert.That(await stop, Is.True);
        Assert.That(
            events,
            Is.EqualTo(new[] { "play:rec-start", "mute", "restore", "play:rec-stop" })
        );
    }

    [Test]
    public async Task StopRecordingAsync_RestoresBeforePlaybackAndPreservesRestoreResult()
    {
        var events = new ConcurrentQueue<string>();
        var playbackStarted = NewCompletionSource();
        var releasePlayback = NewCompletionSource();
        var controller = new RecordingAmbianceController(
            async soundName =>
            {
                events.Enqueue($"play:{soundName}");
                playbackStarted.TrySetResult(true);
                await releasePlayback.Task;
                throw new InvalidOperationException("output unavailable");
            },
            () => true,
            () =>
            {
                events.Enqueue("restore");
                return false;
            },
            _ => events.Enqueue("log")
        );

        var operation = controller.StopRecordingAsync(
            wasMuted: true,
            muteSounds: false
        );
        await playbackStarted.Task;

        Assert.That(events, Is.EqualTo(new[] { "restore", "play:rec-stop" }));
        Assert.That(operation.IsCompleted, Is.False);

        releasePlayback.TrySetResult(true);
        var success = await operation;
        Assert.That(success, Is.False);
        Assert.That(events, Is.EqualTo(new[] { "restore", "play:rec-stop", "log" }));
    }

    [Test]
    public async Task PlaybackTimeout_StopsPlaybackAndAllowsNextOperation()
    {
        var stopped = false;
        var playCount = 0;
        var neverCompletes = NewCompletionSource();
        var controller = new RecordingAmbianceController(
            async _ =>
            {
                if (Interlocked.Increment(ref playCount) == 1)
                {
                    await AudioService.WaitForPlaybackAsync(
                        neverCompletes.Task,
                        TimeSpan.FromMilliseconds(20),
                        () => stopped = true
                    );
                }
            },
            () => true,
            () => true,
            _ => { }
        );

        Assert.That(
            await controller.StartRecordingAsync(shouldMute: true, muteSounds: false),
            Is.True
        );
        Assert.That(stopped, Is.True);
        Assert.That(
            await controller.StopRecordingAsync(wasMuted: true, muteSounds: false),
            Is.True
        );
        Assert.That(playCount, Is.EqualTo(2));
    }

    [TestCase(true)]
    [TestCase(false)]
    public async Task MediaResumeRunsEvenWhenSystemMuteFailed(bool muteSucceeded)
    {
        var events = new ConcurrentQueue<string>();
        var controller = new RecordingAmbianceController(
            sound => { events.Enqueue(sound); return Task.CompletedTask; },
            () => { events.Enqueue("mute"); return muteSucceeded; },
            () => { events.Enqueue("restore"); return true; },
            _ => { },
            enabled => { events.Enqueue($"pause:{enabled}"); return Task.CompletedTask; },
            () => { events.Enqueue("resume"); return Task.CompletedTask; }
        );

        Assert.That(await controller.StartRecordingAsync(true, false), Is.EqualTo(muteSucceeded));
        await controller.StopRecordingAsync(muteSucceeded, false);

        var expected = new List<string> { "rec-start", "mute", "pause:True" };
        if (muteSucceeded) expected.Add("restore");
        expected.AddRange(new[] { "rec-stop", "resume" });
        Assert.That(events, Is.EqualTo(expected));
    }

    [Test]
    public async Task StopDuringPendingMediaPauseWaitsThenResumes()
    {
        var started = NewCompletionSource();
        var release = NewCompletionSource();
        var resumed = false;
        var controller = new RecordingAmbianceController(
            _ => Task.CompletedTask, () => true, () => true, _ => { },
            async _ => { started.TrySetResult(true); await release.Task; },
            () => { resumed = true; return Task.CompletedTask; }
        );

        var start = controller.StartRecordingAsync(true, true);
        await started.Task;
        var stop = controller.StopRecordingAsync(false, true);
        Assert.That(resumed, Is.False);
        release.TrySetResult(true);
        await Task.WhenAll(start, stop);
        Assert.That(resumed, Is.True);
    }

    [Test]
    public async Task RestoreFailureStillResumesMediaAndReleasesTheGate()
    {
        var resumes = 0;
        var controller = new RecordingAmbianceController(
            _ => Task.CompletedTask, () => true,
            () => throw new InvalidOperationException("audio device disconnected"), _ => { },
            _ => Task.CompletedTask,
            () => { resumes++; return Task.CompletedTask; }
        );
        Func<Task> stop = () => controller.StopRecordingAsync(true, true);
        Assert.ThrowsAsync<InvalidOperationException>(stop);
        Assert.That(resumes, Is.EqualTo(1));
        Assert.That(await controller.StartRecordingAsync(false, true).WaitAsync(TimeSpan.FromSeconds(1)), Is.True);
    }

    private static TaskCompletionSource<bool> NewCompletionSource()
    {
        return new TaskCompletionSource<bool>(
            TaskCreationOptions.RunContinuationsAsynchronously
        );
    }
}
