using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;
using Mystique.Core.Contracts;

namespace Mystique.Core.Device;

/// <summary>
/// Owns one WinUSB handle for its lifetime. Not thread-safe by design: exactly one
/// worker thread performs exchanges. <see cref="AbortPendingTransfers"/> is the one
/// member callable from another thread.
/// </summary>
public sealed class MystiqueDevice : IMystiqueDevice
{
    /// <summary>
    /// Applied to all four endpoints at open time. Bulk pipes default to 0, which
    /// means <i>infinite</i>: forgetting this on a reopen hangs the worker forever
    /// with no exception and no log.
    /// </summary>
    private const uint TransferTimeoutMilliseconds = 1500;

    private static readonly byte[] AllPipes =
    [
        MystiquePackets.InitPipeOut,
        MystiquePackets.InitPipeIn,
        MystiquePackets.DataPipeOut,
        MystiquePackets.DataPipeIn,
    ];

    private readonly SafeFileHandle file;
    private readonly PipeInfo[] pipes;
    private IntPtr usb;
    private volatile bool abortRequested;

    private MystiqueDevice(SafeFileHandle file, IntPtr usb, string redactedPath, PipeInfo[] pipes)
    {
        this.file = file;
        this.usb = usb;
        this.pipes = pipes;
        RedactedPath = redactedPath;
    }

    public string RedactedPath { get; }

    /// <summary>
    /// Locates, opens, initializes, and applies pipe policy as one indivisible step.
    /// There is deliberately no way to obtain a device without its policy applied.
    /// </summary>
    public static MystiqueDevice Open()
    {
        string path = DeviceLocator.FindPresentDevicePath()
            ?? throw new MystiqueUsbException(
                UsbFault.Classify(UsbFault.FileNotFound), "locate device");

        SafeFileHandle file = Native.CreateFile(
            path,
            Native.GenericRead | Native.GenericWrite,
            // Permissive sharing is an invariant, not an oversight. Getting there
            // first with a permissive share mode is what makes takeover work in our
            // favour rather than producing a reset war with DeepCool.
            Native.FileShareRead | Native.FileShareWrite,
            IntPtr.Zero,
            Native.OpenExisting,
            Native.FileAttributeNormal | Native.FileFlagOverlapped,
            IntPtr.Zero);

        if (file.IsInvalid)
        {
            int code = Marshal.GetLastWin32Error();
            file.Dispose();
            throw new MystiqueUsbException(UsbFault.Classify(code), "CreateFile");
        }

        if (!Native.WinUsb_Initialize(file, out IntPtr usb))
        {
            int code = Marshal.GetLastWin32Error();
            file.Dispose();
            throw new MystiqueUsbException(UsbFault.Classify(code), "WinUsb_Initialize");
        }

        try
        {
            PipeInfo[] discovered = QueryPipes(usb);
            ApplyPipePolicy(usb);
            return new MystiqueDevice(file, usb, DeviceLocator.RedactSerial(path), discovered);
        }
        catch
        {
            Native.WinUsb_Free(usb);
            file.Dispose();
            throw;
        }
    }

    public IReadOnlyList<PipeInfo> DescribePipes() => pipes;

    public IReadOnlyList<InitStep> EnterMachineInfoMode(byte rotation)
    {
        var steps = new List<InitStep>();

        foreach (InitCommand command in MystiquePackets.BuildInitSequence(rotation))
        {
            byte[] response = Exchange(
                MystiquePackets.InitPipeOut, MystiquePackets.InitPipeIn,
                command.Command, command.Payload);
            steps.Add(new InitStep(command.Command, response));
            Thread.Sleep(command.SettleMilliseconds);
        }

        DeviceStatus status = ReadStatus();
        if (status.Mode != DisplayMode.MachineInfo)
            throw new InvalidOperationException(
                $"Device did not enter Machine Info mode (mode byte 0x{status.RawMode:X2}).");

        return steps;
    }

    public DeviceStatus ReadStatus()
    {
        byte[] response = Exchange(
            MystiquePackets.DataPipeOut, MystiquePackets.DataPipeIn,
            MystiquePackets.CommandStatus, []);
        return MystiquePackets.ParseStatus(response);
    }

    public byte[] SendInfoRequest() =>
        Exchange(
            MystiquePackets.InitPipeOut, MystiquePackets.InitPipeIn,
            MystiquePackets.CommandInfo, []);

    public byte[] SendTelemetry(in TelemetryFrame frame) =>
        Exchange(
            MystiquePackets.DataPipeOut, MystiquePackets.DataPipeIn,
            MystiquePackets.CommandTelemetry, MystiquePackets.BuildDisplayPayload(frame));

    public void Recover(RecoveryAction action)
    {
        if (action is RecoveryAction.None or RecoveryAction.Reopen or RecoveryAction.WaitForArrival)
            return;

        IntPtr handle = usb;
        if (handle == IntPtr.Zero)
            return;

        foreach (byte pipe in AllPipes)
        {
            Native.WinUsb_AbortPipe(handle, pipe);
            Native.WinUsb_FlushPipe(handle, pipe);
            if (action == RecoveryAction.AbortFlushReset)
                Native.WinUsb_ResetPipe(handle, pipe);
        }
    }

    public void AbortPendingTransfers()
    {
        IntPtr handle = usb;
        if (handle == IntPtr.Zero)
            return;

        abortRequested = true;
        foreach (byte pipe in AllPipes)
            Native.WinUsb_AbortPipe(handle, pipe);
    }

    public void Dispose()
    {
        IntPtr handle = Interlocked.Exchange(ref usb, IntPtr.Zero);
        if (handle != IntPtr.Zero)
            Native.WinUsb_Free(handle);

        file.Dispose();
    }

    private byte[] Exchange(byte outPipe, byte inPipe, byte command, ReadOnlySpan<byte> payload)
    {
        IntPtr handle = usb;
        ObjectDisposedException.ThrowIf(handle == IntPtr.Zero, this);

        byte[] packet = MystiquePackets.BuildPacket(command, payload);

        if (!Native.WinUsb_WritePipe(handle, outPipe, packet, (uint)packet.Length,
                out uint written, IntPtr.Zero))
            throw Fail($"write 0x{outPipe:X2} command 0x{command:X2}");

        if (written != packet.Length)
            throw new IOException(
                $"Short write on 0x{outPipe:X2}: {written}/{packet.Length} bytes");

        byte[] response = new byte[64];
        if (!Native.WinUsb_ReadPipe(handle, inPipe, response, (uint)response.Length,
                out uint read, IntPtr.Zero))
            throw Fail($"read 0x{inPipe:X2} command 0x{command:X2}");

        Array.Resize(ref response, checked((int)read));
        MystiquePackets.ValidateResponse(response, command);
        return response;
    }

    /// <summary>
    /// Builds the classified exception. Because pipe policy no longer runs per
    /// exchange, this is the only place a dead device is detected, so it must
    /// classify rather than rethrow blindly.
    /// </summary>
    private MystiqueUsbException Fail(string operation)
    {
        int code = Marshal.GetLastWin32Error();
        return new MystiqueUsbException(UsbFault.Classify(code, abortRequested), operation);
    }

    private static PipeInfo[] QueryPipes(IntPtr usb)
    {
        if (!Native.WinUsb_QueryInterfaceSettings(usb, 0, out UsbInterfaceDescriptor descriptor))
            throw new MystiqueUsbException(
                UsbFault.Classify(Marshal.GetLastWin32Error()), "WinUsb_QueryInterfaceSettings");

        var discovered = new PipeInfo[descriptor.NumEndpoints];
        for (byte index = 0; index < descriptor.NumEndpoints; index++)
        {
            if (!Native.WinUsb_QueryPipe(usb, 0, index, out WinUsbPipeInformation pipe))
                throw new MystiqueUsbException(
                    UsbFault.Classify(Marshal.GetLastWin32Error()), $"WinUsb_QueryPipe({index})");

            discovered[index] = new PipeInfo(pipe.PipeId, pipe.PipeType, pipe.MaximumPacketSize);
        }

        return discovered;
    }

    private static void ApplyPipePolicy(IntPtr usb)
    {
        foreach (byte pipe in AllPipes)
        {
            uint timeout = TransferTimeoutMilliseconds;
            if (!Native.WinUsb_SetPipePolicyUInt32(
                    usb, pipe, Native.PipeTransferTimeout, sizeof(uint), ref timeout))
                throw new MystiqueUsbException(
                    UsbFault.Classify(Marshal.GetLastWin32Error()),
                    $"set PIPE_TRANSFER_TIMEOUT on 0x{pipe:X2}");

            // Defaults to FALSE. Exists precisely for devices that do not preserve
            // data-toggle state across suspend, which is the sleep/resume case.
            byte resetOnResume = 1;
            if (!Native.WinUsb_SetPipePolicyByte(
                    usb, pipe, Native.ResetPipeOnResume, sizeof(byte), ref resetOnResume))
                throw new MystiqueUsbException(
                    UsbFault.Classify(Marshal.GetLastWin32Error()),
                    $"set RESET_PIPE_ON_RESUME on 0x{pipe:X2}");
        }
    }
}
