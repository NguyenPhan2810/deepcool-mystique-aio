namespace Mystique.Core.Contracts;

/// <summary>
/// The five values the Machine Info display shows, already clamped to the ranges
/// the wire format allows. Constructing one cannot produce an out-of-range packet.
/// </summary>
public readonly record struct TelemetryFrame(
    byte CpuTemperatureC,
    byte CpuUsagePercent,
    byte MemoryLoadPercent,
    byte GpuTemperatureC,
    ushort CpuClockMhz)
{
    public static TelemetryFrame FromRaw(
        double cpuTemperature,
        double cpuUsage,
        double memoryLoad,
        double gpuTemperature,
        double cpuClock) =>
        new(
            Clamp(cpuTemperature, 0, 127),
            Clamp(cpuUsage, 0, 100),
            Clamp(memoryLoad, 0, 100),
            Clamp(gpuTemperature, 0, 127),
            (ushort)Math.Clamp((int)Math.Round(cpuClock), 0, ushort.MaxValue));

    private static byte Clamp(double value, int minimum, int maximum) =>
        (byte)Math.Clamp((int)Math.Round(value), minimum, maximum);
}
