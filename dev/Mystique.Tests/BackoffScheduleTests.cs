using Mystique.Core.Device;

namespace Mystique.Tests;

public class BackoffScheduleTests
{
    // Jitter of 0.5 is the midpoint, which maps to no adjustment at all.
    private static Func<double> NoJitter => () => 0.5;

    [Fact]
    public void FastScheduleStaysAtOneSecond()
    {
        var schedule = BackoffSchedule.Fast(NoJitter);

        for (int i = 0; i < 10; i++)
            Assert.Equal(TimeSpan.FromSeconds(1), schedule.NextDelay());
    }

    [Fact]
    public void FastScheduleGivesUpAfterThirtyAttempts()
    {
        var schedule = BackoffSchedule.Fast(NoJitter);

        for (int i = 0; i < 30; i++)
        {
            Assert.False(schedule.Exhausted);
            schedule.NextDelay();
        }

        Assert.True(schedule.Exhausted);
    }

    [Fact]
    public void SlowScheduleDoublesThenCapsAtThirtySeconds()
    {
        var schedule = BackoffSchedule.Slow(NoJitter);

        Assert.Equal(TimeSpan.FromSeconds(1), schedule.NextDelay());
        Assert.Equal(TimeSpan.FromSeconds(2), schedule.NextDelay());
        Assert.Equal(TimeSpan.FromSeconds(4), schedule.NextDelay());
        Assert.Equal(TimeSpan.FromSeconds(8), schedule.NextDelay());
        Assert.Equal(TimeSpan.FromSeconds(16), schedule.NextDelay());
        Assert.Equal(TimeSpan.FromSeconds(30), schedule.NextDelay());
        Assert.Equal(TimeSpan.FromSeconds(30), schedule.NextDelay());
        Assert.Equal(TimeSpan.FromSeconds(30), schedule.NextDelay());
    }

    [Fact]
    public void SlowScheduleNeverGivesUp()
    {
        var schedule = BackoffSchedule.Slow(NoJitter);

        for (int i = 0; i < 1000; i++)
            schedule.NextDelay();

        Assert.False(schedule.Exhausted);
    }

    [Theory]
    [InlineData(0.0, 0.8)] // full negative jitter
    [InlineData(0.5, 1.0)] // midpoint
    [InlineData(1.0, 1.2)] // full positive jitter
    public void JitterSpansPlusOrMinusTwentyPercent(double roll, double expectedMultiplier)
    {
        var schedule = BackoffSchedule.Fast(() => roll);

        TimeSpan delay = schedule.NextDelay();

        Assert.Equal(1000 * expectedMultiplier, delay.TotalMilliseconds, precision: 3);
    }

    [Fact]
    public void JitterIsAppliedToCappedDelaysToo()
    {
        var schedule = BackoffSchedule.Slow(() => 0.0);

        for (int i = 0; i < 5; i++)
            schedule.NextDelay();

        Assert.Equal(TimeSpan.FromSeconds(24), schedule.NextDelay()); // 30 s * 0.8
    }

    [Fact]
    public void ASingleSuccessDoesNotResetTheSchedule()
    {
        var schedule = BackoffSchedule.Slow(NoJitter);
        schedule.NextDelay();
        schedule.NextDelay();

        schedule.RecordSuccess();

        // Still advancing, not restarted: a flapping device must not turn
        // exponential backoff into a permanent one-second hammer.
        Assert.Equal(TimeSpan.FromSeconds(4), schedule.NextDelay());
    }

    [Fact]
    public void ThirtyConsecutiveSuccessesResetTheSchedule()
    {
        var schedule = BackoffSchedule.Slow(NoJitter);
        schedule.NextDelay();
        schedule.NextDelay();
        schedule.NextDelay();

        for (int i = 0; i < BackoffSchedule.SuccessesBeforeReset; i++)
            schedule.RecordSuccess();

        Assert.Equal(0, schedule.ConsecutiveFailures);
        Assert.Equal(TimeSpan.FromSeconds(1), schedule.NextDelay());
    }

    [Fact]
    public void AFailureRestartsTheSuccessRunTowardsReset()
    {
        var schedule = BackoffSchedule.Slow(NoJitter);
        schedule.NextDelay();

        for (int i = 0; i < 29; i++)
            schedule.RecordSuccess();
        schedule.NextDelay();                      // one failure wipes the run of 29
        for (int i = 0; i < 29; i++)
            schedule.RecordSuccess();

        Assert.Equal(2, schedule.ConsecutiveFailures);
    }

    [Fact]
    public void ConsecutiveFailuresCountsAttempts()
    {
        var schedule = BackoffSchedule.Slow(NoJitter);

        Assert.Equal(0, schedule.ConsecutiveFailures);
        schedule.NextDelay();
        Assert.Equal(1, schedule.ConsecutiveFailures);
        schedule.NextDelay();
        Assert.Equal(2, schedule.ConsecutiveFailures);
    }

    [Fact]
    public void ResetClearsEverythingImmediately()
    {
        var schedule = BackoffSchedule.Fast(NoJitter);
        for (int i = 0; i < 30; i++)
            schedule.NextDelay();

        schedule.Reset();

        Assert.False(schedule.Exhausted);
        Assert.Equal(0, schedule.ConsecutiveFailures);
    }

    [Fact]
    public void DefaultJitterStaysWithinTheDocumentedBand()
    {
        var schedule = BackoffSchedule.Fast();

        for (int i = 0; i < 200; i++)
        {
            double ms = schedule.NextDelay().TotalMilliseconds;
            Assert.InRange(ms, 800, 1200);
            schedule.Reset();
        }
    }
}
