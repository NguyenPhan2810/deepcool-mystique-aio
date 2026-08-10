using Mystique.Core.Contracts;
using Mystique.Core.Device;

namespace Mystique.Tests;

public sealed class FakeMystiqueDevice : IMystiqueDevice
{
    public string RedactedPath => @"\\?\USB#VID_3633&PID_0009#<serial>#{guid}";

    public int InitCalls { get; private set; }
    public int TelemetryCalls { get; private set; }
    public int RecoverCalls { get; private set; }
    public int AbortCalls { get; private set; }
    public bool Disposed { get; private set; }
    public List<byte> RotationsRequested { get; } = [];

    /// <summary>Throws on the Nth telemetry call (1-based). Null disables.</summary>
    public Func<int, Exception?>? TelemetryFault { get; set; }

    public Exception? InitFault { get; set; }

    public IReadOnlyList<PipeInfo> DescribePipes() =>
    [
        new(0x01, UsbdPipeType.Bulk, 64), new(0x81, UsbdPipeType.Bulk, 64),
        new(0x02, UsbdPipeType.Bulk, 64), new(0x82, UsbdPipeType.Bulk, 64),
    ];

    public IReadOnlyList<InitStep> EnterMachineInfoMode(byte rotation)
    {
        ObjectDisposedException.ThrowIf(Disposed, this);
        InitCalls++;
        RotationsRequested.Add(rotation);
        if (InitFault is { } fault)
            throw fault;

        return [new InitStep(0x12, [0x55, 0x2E, 0x12, 0x00])];
    }

    public DeviceStatus ReadStatus()
    {
        ObjectDisposedException.ThrowIf(Disposed, this);
        return new DeviceStatus(DisplayMode.MachineInfo, 0xFF, 0);
    }

    public byte[] SendInfoRequest() => [0x55, 0x2E, 0x12, 0x00];

    public byte[] SendTelemetry(in TelemetryFrame frame)
    {
        ObjectDisposedException.ThrowIf(Disposed, this);
        TelemetryCalls++;
        if (TelemetryFault?.Invoke(TelemetryCalls) is { } fault)
            throw fault;

        return [0x55, 0x2E, 0x01, 0x00];
    }

    public void Recover(RecoveryAction action) => RecoverCalls++;

    public void AbortPendingTransfers() => AbortCalls++;

    public void Dispose() => Disposed = true;
}

public sealed class FakeSensorSource : ISensorSource
{
    private int reads;

    public bool Disposed { get; private set; }

    public int Reads => reads;

    /// <summary>Throws on the Nth read (1-based). Null disables.</summary>
    public Func<int, Exception?>? ReadFault { get; set; }

    public async Task<SensorSample> ReadAsync(CancellationToken cancellationToken)
    {
        // Genuinely yields. A synchronously-completing fake would let the supervisor
        // loop spin without ever suspending, starving the test that is watching it.
        // The real bridge suspends here on pipe I/O.
        await Task.Yield();

        cancellationToken.ThrowIfCancellationRequested();
        int n = Interlocked.Increment(ref reads);
        if (ReadFault?.Invoke(n) is { } fault)
            throw fault;

        return new SensorSample(55, 12, 41, 48, 4600);
    }

    public void Dispose() => Disposed = true;
}
