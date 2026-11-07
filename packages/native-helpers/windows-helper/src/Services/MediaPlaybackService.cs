using System;
using System.Collections.Generic;
using System.Threading;
using System.Threading.Tasks;
using Windows.Media.Control;

namespace WindowsHelper.Services;

internal interface IMediaPlaybackSession : IDisposable
{
    string AppId { get; }
    bool IsPlaying { get; }
    bool IsPaused { get; }
    bool CanPause { get; }
    bool CanPlay { get; }
    event Action? ResumeInvalidated;
    Task<bool> PauseAsync(CancellationToken cancellationToken);
    Task<bool> PlayAsync(CancellationToken cancellationToken);
}

// Called under RecordingAmbianceController's gate; no change to the shared RPC contract.
internal sealed class MediaPlaybackService
{
    private static readonly TimeSpan OperationTimeout = TimeSpan.FromSeconds(2);
    private readonly Func<CancellationToken, Task<IReadOnlyList<IMediaPlaybackSession>>> getSessions;
    private readonly Action<string> log;
    private readonly List<PausedPlayback> paused = new();
    private int activeRecordings;

    internal MediaPlaybackService(Action<string> log)
        : this(GetWindowsSessionsAsync, log) { }

    internal MediaPlaybackService(
        Func<CancellationToken, Task<IReadOnlyList<IMediaPlaybackSession>>> getSessions,
        Action<string> log)
    {
        this.getSessions = getSessions;
        this.log = log;
    }

    internal async Task PauseAsync(bool enabled)
    {
        // A successor recording may start before its predecessor has fully closed.
        if (activeRecordings++ > 0 || !enabled) return;
        using var timeout = new CancellationTokenSource(OperationTimeout);
        try
        {
            var sessions = await getSessions(timeout.Token);
            foreach (var session in sessions)
            {
                var retained = false;
                try
                {
                    if (timeout.IsCancellationRequested || !session.IsPlaying || !session.CanPause)
                        continue;
                    if (await session.PauseAsync(timeout.Token))
                    {
                        paused.Add(new PausedPlayback(session));
                        retained = true;
                        log($"[MediaPlayback] Paused {session.AppId}");
                    }
                }
                catch (Exception ex)
                {
                    log($"[MediaPlayback] Could not pause player: {ex.Message}");
                }
                finally
                {
                    if (!retained)
                    {
                        try { session.Dispose(); }
                        catch (Exception ex) { log($"[MediaPlayback] Player cleanup failed: {ex.Message}"); }
                    }
                }
            }
        }
        catch (Exception ex)
        {
            // Unsupported/unavailable media controls must never block dictation or muting.
            log($"[MediaPlayback] Media controls unavailable: {ex.Message}");
        }
    }

    internal async Task ResumeAsync()
    {
        if (activeRecordings == 0 || --activeRecordings > 0) return;
        using var timeout = new CancellationTokenSource(OperationTimeout);
        foreach (var playback in paused)
        {
            try
            {
                var session = playback.Session;
                if (!timeout.IsCancellationRequested && playback.CanResume && session.IsPaused && session.CanPlay)
                {
                    var resumed = await session.PlayAsync(timeout.Token);
                    log($"[MediaPlayback] Resume {session.AppId}: {resumed}");
                }
            }
            catch (Exception ex)
            {
                log($"[MediaPlayback] Could not resume player: {ex.Message}");
            }
            finally
            {
                try { playback.Dispose(); }
                catch (Exception ex) { log($"[MediaPlayback] Player cleanup failed: {ex.Message}"); }
            }
        }
        paused.Clear();
    }

    private sealed class PausedPlayback : IDisposable
    {
        private int invalidated;
        internal IMediaPlaybackSession Session { get; }
        internal bool CanResume => Volatile.Read(ref invalidated) == 0;

        internal PausedPlayback(IMediaPlaybackSession session)
        {
            Session = session;
            Session.ResumeInvalidated += Invalidate;
        }

        private void Invalidate() => Interlocked.Exchange(ref invalidated, 1);

        public void Dispose()
        {
            Session.ResumeInvalidated -= Invalidate;
            Session.Dispose();
        }
    }

    private static async Task<IReadOnlyList<IMediaPlaybackSession>> GetWindowsSessionsAsync(
        CancellationToken cancellationToken)
    {
        var manager = await GlobalSystemMediaTransportControlsSessionManager.RequestAsync()
            .AsTask(cancellationToken);
        var sessions = new List<IMediaPlaybackSession>();
        foreach (var session in manager.GetSessions())
            sessions.Add(new WindowsMediaPlaybackSession(session));
        return sessions;
    }
}

internal sealed class WindowsMediaPlaybackSession : IMediaPlaybackSession
{
    private readonly GlobalSystemMediaTransportControlsSession session;
    public string AppId => session.SourceAppUserModelId;
    public bool IsPlaying => session.GetPlaybackInfo().PlaybackStatus == GlobalSystemMediaTransportControlsSessionPlaybackStatus.Playing;
    public bool IsPaused => session.GetPlaybackInfo().PlaybackStatus == GlobalSystemMediaTransportControlsSessionPlaybackStatus.Paused;
    public bool CanPause => session.GetPlaybackInfo().Controls.IsPauseEnabled;
    public bool CanPlay => session.GetPlaybackInfo().Controls.IsPlayEnabled;
    public event Action? ResumeInvalidated;

    internal WindowsMediaPlaybackSession(GlobalSystemMediaTransportControlsSession session)
    {
        this.session = session;
        session.PlaybackInfoChanged += OnPlaybackChanged;
        session.MediaPropertiesChanged += OnMediaChanged;
    }

    public Task<bool> PauseAsync(CancellationToken cancellationToken) =>
        session.TryPauseAsync().AsTask(cancellationToken);

    public Task<bool> PlayAsync(CancellationToken cancellationToken) =>
        session.TryPlayAsync().AsTask(cancellationToken);

    private void OnPlaybackChanged(GlobalSystemMediaTransportControlsSession sender, PlaybackInfoChangedEventArgs args)
    {
        try
        {
            if (!IsPaused) ResumeInvalidated?.Invoke();
        }
        catch
        {
            ResumeInvalidated?.Invoke();
        }
    }

    private void OnMediaChanged(GlobalSystemMediaTransportControlsSession sender, MediaPropertiesChangedEventArgs args) =>
        ResumeInvalidated?.Invoke();

    public void Dispose()
    {
        session.PlaybackInfoChanged -= OnPlaybackChanged;
        session.MediaPropertiesChanged -= OnMediaChanged;
    }
}
