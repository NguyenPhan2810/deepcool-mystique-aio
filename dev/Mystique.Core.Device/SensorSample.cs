namespace Mystique.Core.Device;

public sealed record SensorSample(
    double CpuTemperature,
    double CpuUsage,
    double MemoryLoad,
    double GpuTemperature,
    double CpuClock);

/// <summary>
/// A source of sensor readings. An interface so the supervisor state machine can
/// be driven from a fake in tests, and so a non-DeepCool source can be substituted
/// later without touching the supervisor.
/// </summary>
public interface ISensorSource : IDisposable
{
    /// <summary>
    /// Completes when the next sample is pushed. This is a subscription, not a
    /// poll: the bridge pushes at its own cadence (~1.08 s measured), which is what
    /// sets the LCD refresh rate.
    /// </summary>
    Task<SensorSample> ReadAsync(CancellationToken cancellationToken);
}
