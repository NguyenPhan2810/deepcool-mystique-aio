using Mystique.Core.Contracts;

namespace Mystique.Core.Device;

/// <summary>One command of the init handshake and the device's reply to it.</summary>
public readonly record struct InitStep(byte Command, byte[] Response);

public readonly record struct PipeInfo(byte PipeId, UsbdPipeType Type, ushort MaximumPacketSize);

/// <summary>
/// The device surface the supervisor depends on. An interface so the state
/// machine can be tested against a fake without hardware.
/// </summary>
public interface IMystiqueDevice : IDisposable
{
    /// <summary>Interface path with the per-device serial masked. Safe to log.</summary>
    string RedactedPath { get; }

    IReadOnlyList<PipeInfo> DescribePipes();

    /// <summary>
    /// Runs the 13-command handshake and verifies the device reports Machine Info
    /// mode. Must be re-run after every reopen: a reopened device is not in
    /// Machine Info mode, and telemetry writes would be ACKed while displaying nothing.
    /// </summary>
    IReadOnlyList<InitStep> EnterMachineInfoMode(byte rotation);

    DeviceStatus ReadStatus();

    /// <summary>Sends command 0x12. The reply body carries a per-device identifier.</summary>
    byte[] SendInfoRequest();

    byte[] SendTelemetry(in TelemetryFrame frame);

    void Recover(RecoveryAction action);

    /// <summary>Cancels in-flight transfers so a blocked worker can unwind promptly.</summary>
    void AbortPendingTransfers();
}
