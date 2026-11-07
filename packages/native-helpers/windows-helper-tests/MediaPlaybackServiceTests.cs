using NUnit.Framework;
using WindowsHelper.Services;

namespace WindowsHelper.Tests;

[TestFixture]
public class MediaPlaybackServiceTests
{
    [Test]
    public async Task OnlyResumesPlayersThatWerePlayingAndSuccessfullyPaused()
    {
        var playing = new FakeSession();
        var alreadyPaused = new FakeSession { IsPlaying = false, IsPaused = true };
        var unsupported = new FakeSession { CanPause = false };
        var rejected = new FakeSession { PauseResult = false };
        var service = Create(playing, alreadyPaused, unsupported, rejected);

        await service.PauseAsync(true);
        Assert.That(playing.IsPaused, Is.True);
        await service.ResumeAsync();
        await service.ResumeAsync();

        using (Assert.EnterMultipleScope())
        {
            Assert.That(playing.PlayCalls, Is.EqualTo(1));
            Assert.That(alreadyPaused.PauseCalls, Is.Zero);
            Assert.That(alreadyPaused.PlayCalls, Is.Zero);
            Assert.That(unsupported.PauseCalls, Is.Zero);
            Assert.That(rejected.PlayCalls, Is.Zero);
            Assert.That(new[] { playing, alreadyPaused, unsupported, rejected }.All(s => s.Disposed), Is.True);
        }
    }

    [Test]
    public async Task DisabledPreferenceDoesNotQueryOrControlPlayers()
    {
        var service = new MediaPlaybackService(_ => throw new Exception("must not query"), _ => Assert.Fail());
        await service.PauseAsync(false);
        await service.ResumeAsync();
    }

    [Test]
    public async Task OverlappingRecordingsWaitForTheLastStop()
    {
        var session = new FakeSession();
        var service = Create(session);
        await service.PauseAsync(true);
        await service.PauseAsync(true);
        await service.ResumeAsync();
        Assert.That(session.PlayCalls, Is.Zero);
        await service.ResumeAsync();
        Assert.That(session.PauseCalls, Is.EqualTo(1));
        Assert.That(session.PlayCalls, Is.EqualTo(1));
    }

    [Test]
    public async Task NewDictationTakesANewSnapshot()
    {
        var sessions = new Queue<FakeSession>(new[] { new FakeSession(), new FakeSession() });
        var service = new MediaPlaybackService(
            _ => Task.FromResult<IReadOnlyList<IMediaPlaybackSession>>(new[] { sessions.Dequeue() }), _ => { });
        await service.PauseAsync(true);
        await service.ResumeAsync();
        await service.PauseAsync(true);
        await service.ResumeAsync();
        Assert.That(sessions, Is.Empty);
    }

    [TestCase(false)]
    [TestCase(true)]
    public async Task DoesNotOverrideUserChangesOrResumeAnotherTrack(bool changedMedia)
    {
        var session = new FakeSession();
        var service = Create(session);
        await service.PauseAsync(true);
        // The native adapter invalidates on a track change or playback leaving Paused.
        session.InvalidateResume();
        if (!changedMedia)
        {
            session.IsPlaying = true;
            session.IsPaused = false;
            session.IsPlaying = false;
            session.IsPaused = true;
        }
        await service.ResumeAsync();
        Assert.That(session.PlayCalls, Is.Zero);
        Assert.That(session.Disposed, Is.True);
    }

    [Test]
    public async Task DoesNotResumeStoppedOrUncontrollablePlayers()
    {
        var stopped = new FakeSession();
        var uncontrollable = new FakeSession();
        var service = Create(stopped, uncontrollable);
        await service.PauseAsync(true);
        stopped.IsPaused = false;
        uncontrollable.CanPlay = false;
        await service.ResumeAsync();
        Assert.That(stopped.PlayCalls + uncontrollable.PlayCalls, Is.Zero);
    }

    [Test]
    public async Task AClosedOrBrokenPlayerDoesNotPreventOtherPlayersFromResuming()
    {
        var broken = new FakeSession { PauseError = true };
        var closed = new FakeSession { PlayError = true };
        var healthy = new FakeSession();
        var service = Create(broken, closed, healthy);
        await service.PauseAsync(true);
        await service.ResumeAsync();
        Assert.That(healthy.PlayCalls, Is.EqualTo(1));
        Assert.That(new[] { broken, closed, healthy }.All(s => s.Disposed), Is.True);
    }

    [Test]
    public async Task DiscoveryFailureIsBestEffortAndNextDictationCanRetry()
    {
        var attempts = 0;
        var service = new MediaPlaybackService(_ =>
        {
            attempts++;
            throw new InvalidOperationException("media controls unavailable");
        }, _ => { });
        await service.PauseAsync(true);
        await service.ResumeAsync();
        await service.PauseAsync(true);
        await service.ResumeAsync();
        Assert.That(attempts, Is.EqualTo(2));
    }

    private static MediaPlaybackService Create(params FakeSession[] sessions) =>
        new(_ => Task.FromResult<IReadOnlyList<IMediaPlaybackSession>>(sessions), _ => { });

    [Test]
    public async Task UnresponsiveDiscoveryHasABoundedWaitAndCanStillStop()
    {
        var service = new MediaPlaybackService(async token =>
        {
            await Task.Delay(Timeout.Infinite, token);
            return Array.Empty<IMediaPlaybackSession>();
        }, _ => { });
        await service.PauseAsync(true).WaitAsync(TimeSpan.FromSeconds(5));
        await service.ResumeAsync();
    }

    private sealed class FakeSession : IMediaPlaybackSession
    {
        public string AppId => "test-browser";
        public bool IsPlaying { get; set; } = true;
        public bool IsPaused { get; set; }
        public bool CanPause { get; set; } = true;
        public bool CanPlay { get; set; } = true;
        public bool PauseResult { get; set; } = true;
        public bool PauseError { get; set; }
        public bool PlayError { get; set; }
        public int PauseCalls { get; private set; }
        public int PlayCalls { get; private set; }
        public bool Disposed { get; private set; }
        public event Action? ResumeInvalidated;

        public Task<bool> PauseAsync(CancellationToken cancellationToken)
        {
            PauseCalls++;
            if (PauseError) throw new InvalidOperationException("player closed");
            if (PauseResult) { IsPlaying = false; IsPaused = true; }
            return Task.FromResult(PauseResult);
        }

        public Task<bool> PlayAsync(CancellationToken cancellationToken)
        {
            PlayCalls++;
            if (PlayError) throw new InvalidOperationException("player closed");
            IsPlaying = true;
            IsPaused = false;
            return Task.FromResult(true);
        }

        public void InvalidateResume() => ResumeInvalidated?.Invoke();
        public void Dispose() => Disposed = true;
    }
}
