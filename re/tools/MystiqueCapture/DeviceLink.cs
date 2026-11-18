using System.ComponentModel;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;
using Mystique.Core.Device;

namespace MystiqueCapture;

/// <summary>One exchange's raw outcome. A timeout is data, not an exception.</summary>
internal readonly record struct ExchangeOutcome(bool Received, byte[] Response, int Win32Error, string? Error);

/// <summary>
/// Owns one WinUSB handle for the lifetime of a capture run.
/// Mirrors the open/policy behaviour of Mystique.Core.Device.MystiqueDevice
/// (permissive share mode, 1500 ms transfer timeout, RESET_PIPE_ON_RESUME)
/// so the device sees the same host behaviour as the service does.
/// </summary>
internal sealed class DeviceLink : IDisposable
{
    private const uint TransferTimeoutMilliseconds = 1500;

    private static readonly byte[] AllPipes = [0x01, 0x81, 0x02, 0x82];

    private readonly SafeFileHandle file;
    private IntPtr usb;

    private DeviceLink(SafeFileHandle file, IntPtr usb, IReadOnlyList<PipeInfo> pipes, string redactedPath)
    {
        this.file = file;
        this.usb = usb;
        Pipes = pipes;
        RedactedPath = redactedPath;
    }

    internal sealed record PipeInfo(byte PipeId, string Type, ushort MaximumPacketSize);

    public IReadOnlyList<PipeInfo> Pipes { get; }

    public string RedactedPath { get; }

    public static DeviceLink Open()
    {
        // Empirically, a reopen within a second of the previous handle closing can
        // fail with ERROR_ACCESS_DENIED (5) while the device settles. That is
        // transient, so retry briefly before declaring the device busy.
        const int attempts = 12;
        for (int attempt = 1; ; attempt++)
        {
            try
            {
                return OpenOnce();
            }
            catch (Win32Exception ex) when (ex.NativeErrorCode is 5 or 2 && attempt < attempts)
            {
                Thread.Sleep(500);
            }
        }
    }

    private static DeviceLink OpenOnce()
    {
        string path = DeviceLocator.FindPresentDevicePath()
            ?? throw new InvalidOperationException("MYSTIQUE device not present (VID_3633&PID_0009).");

        SafeFileHandle file = Native.CreateFile(
            path,
            Native.GenericRead | Native.GenericWrite,
            Native.FileShareRead | Native.FileShareWrite,
            IntPtr.Zero,
            Native.OpenExisting,
            Native.FileAttributeNormal | Native.FileFlagOverlapped,
            IntPtr.Zero);

        if (file.IsInvalid)
        {
            int code = Marshal.GetLastWin32Error();
            file.Dispose();
            throw new Win32Exception(code, $"CreateFile failed ({code})");
        }

        if (!Native.WinUsb_Initialize(file, out IntPtr usb))
        {
            int code = Marshal.GetLastWin32Error();
            file.Dispose();
            throw new Win32Exception(code, $"WinUsb_Initialize failed ({code})");
        }

        try
        {
            var pipes = QueryPipes(usb);
            foreach (byte pipe in AllPipes)
            {
                uint timeout = TransferTimeoutMilliseconds;
                if (!Native.WinUsb_SetPipePolicyUInt32(
                        usb, pipe, Native.PipeTransferTimeout, sizeof(uint), ref timeout))
                    throw new Win32Exception(
                        Marshal.GetLastWin32Error(), $"set PIPE_TRANSFER_TIMEOUT on 0x{pipe:X2}");

                byte resetOnResume = 1;
                if (!Native.WinUsb_SetPipePolicyByte(
                        usb, pipe, Native.ResetPipeOnResume, sizeof(byte), ref resetOnResume))
                    throw new Win32Exception(
                        Marshal.GetLastWin32Error(), $"set RESET_PIPE_ON_RESUME on 0x{pipe:X2}");
            }

            return new DeviceLink(file, usb, pipes, DeviceLocator.RedactSerial(path));
        }
        catch
        {
            Native.WinUsb_Free(usb);
            file.Dispose();
            throw;
        }
    }

    /// <summary>
    /// Writes one 48-byte packet and reads at most one 64-byte reply.
    /// Never throws on a transfer fault; the caller records the outcome.
    /// </summary>
    public ExchangeOutcome Exchange(byte outPipe, byte inPipe, byte[] packet)
    {
        IntPtr handle = usb;
        if (handle == IntPtr.Zero)
            return new ExchangeOutcome(false, [], 0, "handle disposed");

        if (!Native.WinUsb_WritePipe(handle, outPipe, packet, (uint)packet.Length, out uint written, IntPtr.Zero))
        {
            int code = Marshal.GetLastWin32Error();
            return new ExchangeOutcome(false, [], code, $"write failed: {new Win32Exception(code).Message}");
        }

        if (written != packet.Length)
            return new ExchangeOutcome(false, [], 0, $"short write {written}/{packet.Length}");

        return Read(inPipe);
    }

    /// <summary>Reads one reply without writing anything first.</summary>
    public ExchangeOutcome Read(byte inPipe)
    {
        IntPtr handle = usb;
        if (handle == IntPtr.Zero)
            return new ExchangeOutcome(false, [], 0, "handle disposed");

        byte[] response = new byte[64];
        if (!Native.WinUsb_ReadPipe(handle, inPipe, response, (uint)response.Length, out uint read, IntPtr.Zero))
        {
            int code = Marshal.GetLastWin32Error();
            return new ExchangeOutcome(false, [], code, $"read failed: {new Win32Exception(code).Message}");
        }

        if (read > response.Length)
            return new ExchangeOutcome(false, [], 0, $"oversized read {read}");

        return new ExchangeOutcome(true, response[..(int)read], 0, null);
    }

    /// <summary>
    /// Aborts, flushes and resets every pipe. Non-destructive host-side recovery;
    /// the same sequence Mystique.Core.Device.Recover uses.
    /// </summary>
    public IReadOnlyList<string> ResetPipes()
    {
        var log = new List<string>();
        IntPtr handle = usb;
        if (handle == IntPtr.Zero)
            return ["handle disposed"];

        foreach (byte pipe in AllPipes)
        {
            bool aborted = Native.WinUsb_AbortPipe(handle, pipe);
            bool flushed = Native.WinUsb_FlushPipe(handle, pipe);
            bool reset = Native.WinUsb_ResetPipe(handle, pipe);
            log.Add($"0x{pipe:X2}: abort={aborted} flush={flushed} reset={reset}");
        }

        return log;
    }

    /// <summary>USB device reset (WinUsb_ResetDevice). Does not touch flash.</summary>
    public bool ResetDevice()
    {
        IntPtr handle = usb;
        return handle != IntPtr.Zero && Native.WinUsb_ResetDevice(handle);
    }

    /// <summary>Issues SET_INTERFACE(0, alt 0); standard request, resets endpoints.</summary>
    public bool SetInterfaceZero()
    {
        IntPtr handle = usb;
        return handle != IntPtr.Zero && Native.WinUsb_SetCurrentAlternateSetting(handle, 0);
    }

    /// <summary>Issues a standard SET_CONFIGURATION(configuration=1) control request.</summary>
    public bool SetConfiguration(byte configuration)
    {
        IntPtr handle = usb;
        if (handle == IntPtr.Zero)
            return false;

        var setup = new Native.WinUsbSetupPacket
        {
            RequestType = 0x00,
            Request = 0x09,
            Value = configuration,
            Index = 0,
            Length = 0,
        };

        return Native.WinUsb_ControlTransfer(handle, setup, [], 0, out _, IntPtr.Zero);
    }

    /// <summary>Standard IN control request. Returns (ok, data, win32 error).</summary>
    public (bool Ok, byte[] Data, int Error) ControlIn(
        byte requestType, byte request, ushort value, ushort index, ushort length)
    {
        IntPtr handle = usb;
        if (handle == IntPtr.Zero)
            return (false, [], 0);

        var setup = new Native.WinUsbSetupPacket
        {
            RequestType = requestType,
            Request = request,
            Value = value,
            Index = index,
            Length = length,
        };

        byte[] buffer = new byte[Math.Max(1, (int)length)];
        if (!Native.WinUsb_ControlTransfer(handle, setup, buffer, length, out uint transferred, IntPtr.Zero))
            return (false, [], Marshal.GetLastWin32Error());

        return (true, buffer[..(int)Math.Min(transferred, buffer.Length)], 0);
    }

    public byte CurrentAlternateSetting()
    {
        IntPtr handle = usb;
        return handle != IntPtr.Zero && Native.WinUsb_GetCurrentAlternateSetting(handle, out byte setting)
            ? setting
            : (byte)0xFF;
    }

    /// <summary>Standard CLEAR_FEATURE(ENDPOINT_HALT) for one endpoint.</summary>
    public bool ClearEndpointHalt(byte endpointAddress)
    {
        IntPtr handle = usb;
        if (handle == IntPtr.Zero)
            return false;

        var setup = new Native.WinUsbSetupPacket
        {
            RequestType = 0x02,
            Request = 0x01,
            Value = 0x0000,
            Index = endpointAddress,
            Length = 0,
        };

        return Native.WinUsb_ControlTransfer(handle, setup, [], 0, out _, IntPtr.Zero);
    }

    public void Dispose()
    {
        IntPtr handle = Interlocked.Exchange(ref usb, IntPtr.Zero);
        if (handle != IntPtr.Zero)
            Native.WinUsb_Free(handle);

        file.Dispose();
    }

    private static IReadOnlyList<PipeInfo> QueryPipes(IntPtr usb)
    {
        if (!Native.WinUsb_QueryInterfaceSettings(usb, 0, out UsbInterfaceDescriptor descriptor))
            throw new Win32Exception(Marshal.GetLastWin32Error(), "WinUsb_QueryInterfaceSettings failed");

        var discovered = new List<PipeInfo>(descriptor.NumEndpoints);
        for (byte index = 0; index < descriptor.NumEndpoints; index++)
        {
            if (!Native.WinUsb_QueryPipe(usb, 0, index, out WinUsbPipeInformation pipe))
                throw new Win32Exception(Marshal.GetLastWin32Error(), $"WinUsb_QueryPipe({index}) failed");

            discovered.Add(new PipeInfo(pipe.PipeId, pipe.PipeType.ToString(), pipe.MaximumPacketSize));
        }

        return discovered;
    }
}
