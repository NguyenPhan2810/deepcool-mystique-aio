namespace Mystique.Core.Device;

public enum LogLevel
{
    Debug,
    Info,
    Warn,
    Error,
}

/// <summary>
/// Decides whether a failed attempt is worth a log line, based on how long the
/// failure has been going on rather than how many attempts have been made.
/// A device unplugged for three days yields roughly 72 lines instead of 8,600.
/// </summary>
public sealed class FailureEscalator(TimeProvider time)
{
    private static readonly TimeSpan[] Thresholds =
    [
        TimeSpan.Zero,
        TimeSpan.FromMinutes(1),
        TimeSpan.FromMinutes(5),
    ];

    private static readonly TimeSpan SteadyStateInterval = TimeSpan.FromHours(1);

    private readonly TimeProvider time = time;
    private DateTimeOffset? failingSince;
    private DateTimeOffset lastLogged;
    private int thresholdsCrossed;

    public TimeSpan FailingFor =>
        failingSince is { } since ? time.GetUtcNow() - since : TimeSpan.Zero;

    /// <summary>Returns the level to log at, or null if this failure should stay silent.</summary>
    public LogLevel? OnFailure()
    {
        DateTimeOffset now = time.GetUtcNow();

        if (failingSince is null)
        {
            failingSince = now;
            lastLogged = now;
            thresholdsCrossed = 1;
            return LogLevel.Warn;
        }

        TimeSpan elapsed = now - failingSince.Value;

        if (thresholdsCrossed < Thresholds.Length && elapsed >= Thresholds[thresholdsCrossed])
        {
            thresholdsCrossed++;
            lastLogged = now;
            return LogLevel.Error;
        }

        if (thresholdsCrossed >= Thresholds.Length && now - lastLogged >= SteadyStateInterval)
        {
            lastLogged = now;
            return LogLevel.Error;
        }

        return null;
    }

    public void OnSuccess()
    {
        failingSince = null;
        thresholdsCrossed = 0;
    }
}
