namespace Mystique.Core.Device;

/// <summary>
/// Retry pacing. Two factory schedules exist because the two situations are not
/// alike.
///
/// <para><b>Fast</b> is for expected conditions - waiting for the sensor bridge at
/// boot, device arrival after a replug, resume from sleep, reopening after DeepCool
/// exits. These clear in seconds. An exponential curve here would leave the panel
/// dark for 30 s at every boot.</para>
///
/// <para><b>Slow</b> is for genuine faults. The 30 s cap is deliberate and low: a
/// retry costs one CreateFile plus a 2 s pipe connect, so backoff here protects
/// against log spam and pointless wakeups, nothing else. A multi-minute cap would
/// mean the LCD stays dead for minutes after the user closes DeepCool, which reads
/// as broken.</para>
///
/// <para>Jitter is not thundering-herd boilerplate. Two deterministic one-second
/// retry loops competing for an <i>exclusive</i> handle can lock-step and livelock,
/// each grabbing and losing it in alternation.</para>
/// </summary>
public sealed class BackoffSchedule
{
    /// <summary>
    /// Resetting on the first success would turn exponential backoff into a
    /// permanent one-second hammer against a flapping device. Roughly 33 s of
    /// clean operation is required instead.
    /// </summary>
    public const int SuccessesBeforeReset = 30;

    private const double JitterFraction = 0.2;

    private readonly TimeSpan[] steps;
    private readonly int maxAttempts;
    private readonly Func<double> jitter;
    private int attempt;
    private int consecutiveSuccesses;

    /// <param name="steps">Delay per attempt; the last entry is the cap.</param>
    /// <param name="maxAttempts">0 means never exhausted.</param>
    /// <param name="jitter">Returns a value in [0, 1). Injected for deterministic tests.</param>
    public BackoffSchedule(TimeSpan[] steps, int maxAttempts, Func<double> jitter)
    {
        ArgumentNullException.ThrowIfNull(steps);
        if (steps.Length == 0)
            throw new ArgumentException("At least one step is required.", nameof(steps));

        this.steps = steps;
        this.maxAttempts = maxAttempts;
        this.jitter = jitter ?? throw new ArgumentNullException(nameof(jitter));
    }

    public static BackoffSchedule Fast(Func<double>? jitter = null) =>
        new([TimeSpan.FromSeconds(1)], maxAttempts: 30, jitter ?? DefaultJitter);

    public static BackoffSchedule Slow(Func<double>? jitter = null) =>
        new(
            [
                TimeSpan.FromSeconds(1),
                TimeSpan.FromSeconds(2),
                TimeSpan.FromSeconds(4),
                TimeSpan.FromSeconds(8),
                TimeSpan.FromSeconds(16),
                TimeSpan.FromSeconds(30),
            ],
            maxAttempts: 0,
            jitter ?? DefaultJitter);

    public int ConsecutiveFailures => attempt;

    public bool Exhausted => maxAttempts > 0 && attempt >= maxAttempts;

    /// <summary>Records a failed attempt and returns how long to wait before the next one.</summary>
    public TimeSpan NextDelay()
    {
        TimeSpan baseDelay = steps[Math.Min(attempt, steps.Length - 1)];
        attempt++;
        consecutiveSuccesses = 0;

        // A jitter roll in [0,1) maps to a multiplier in [0.8, 1.2).
        double multiplier = 1.0 + ((jitter() - 0.5) * 2.0 * JitterFraction);
        return TimeSpan.FromMilliseconds(baseDelay.TotalMilliseconds * multiplier);
    }

    public void RecordSuccess()
    {
        if (attempt == 0)
            return;

        consecutiveSuccesses++;
        if (consecutiveSuccesses >= SuccessesBeforeReset)
            Reset();
    }

    public void Reset()
    {
        attempt = 0;
        consecutiveSuccesses = 0;
    }

    private static double DefaultJitter() => Random.Shared.NextDouble();
}
