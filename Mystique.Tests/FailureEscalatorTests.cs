using Mystique.Core.Device;

namespace Mystique.Tests;

public class FailureEscalatorTests
{
    [Fact]
    public void FirstFailureWarns()
    {
        var time = new FakeTimeProvider();
        var escalator = new FailureEscalator(time);

        Assert.Equal(LogLevel.Warn, escalator.OnFailure());
    }

    [Fact]
    public void FurtherFailuresInsideTheFirstMinuteAreSilent()
    {
        var time = new FakeTimeProvider();
        var escalator = new FailureEscalator(time);
        escalator.OnFailure();

        for (int i = 0; i < 50; i++)
        {
            time.Advance(TimeSpan.FromSeconds(1));
            Assert.Null(escalator.OnFailure());
        }
    }

    [Fact]
    public void OneMinuteOfFailureEscalatesToError()
    {
        var time = new FakeTimeProvider();
        var escalator = new FailureEscalator(time);
        escalator.OnFailure();

        time.Advance(TimeSpan.FromMinutes(1));

        Assert.Equal(LogLevel.Error, escalator.OnFailure());
    }

    [Fact]
    public void FiveMinutesOfFailureLogsAgain()
    {
        var time = new FakeTimeProvider();
        var escalator = new FailureEscalator(time);
        escalator.OnFailure();
        time.Advance(TimeSpan.FromMinutes(1));
        escalator.OnFailure();

        time.Advance(TimeSpan.FromMinutes(4));

        Assert.Equal(LogLevel.Error, escalator.OnFailure());
    }

    [Fact]
    public void AfterFiveMinutesItSettlesToHourly()
    {
        var time = new FakeTimeProvider();
        var escalator = new FailureEscalator(time);
        escalator.OnFailure();
        time.Advance(TimeSpan.FromMinutes(5));
        escalator.OnFailure();
        escalator.OnFailure();

        time.Advance(TimeSpan.FromMinutes(59));
        Assert.Null(escalator.OnFailure());

        time.Advance(TimeSpan.FromMinutes(1));
        Assert.Equal(LogLevel.Error, escalator.OnFailure());
    }

    [Fact]
    public void ThreeDaysOfFailureStaysUnderOneHundredLines()
    {
        var time = new FakeTimeProvider();
        var escalator = new FailureEscalator(time);
        int logged = 0;

        // One attempt every 30 s for 72 hours.
        for (int i = 0; i < 72 * 120; i++)
        {
            if (escalator.OnFailure() is not null)
                logged++;
            time.Advance(TimeSpan.FromSeconds(30));
        }

        Assert.InRange(logged, 70, 80);
    }

    [Fact]
    public void SuccessClearsTheEscalationState()
    {
        var time = new FakeTimeProvider();
        var escalator = new FailureEscalator(time);
        escalator.OnFailure();
        time.Advance(TimeSpan.FromMinutes(10));
        escalator.OnFailure();

        escalator.OnSuccess();

        Assert.Equal(TimeSpan.Zero, escalator.FailingFor);
        Assert.Equal(LogLevel.Warn, escalator.OnFailure());
    }

    [Fact]
    public void FailingForReportsElapsedTime()
    {
        var time = new FakeTimeProvider();
        var escalator = new FailureEscalator(time);
        escalator.OnFailure();

        time.Advance(TimeSpan.FromMinutes(3));

        Assert.Equal(TimeSpan.FromMinutes(3), escalator.FailingFor);
    }
}
