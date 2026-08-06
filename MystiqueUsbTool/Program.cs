using System.ComponentModel;
using System.IO.Pipes;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.Json;
using Microsoft.Win32;
using Microsoft.Win32.SafeHandles;

internal static class Program
{
    private const string DeviceRegistryPath =
        @"SYSTEM\CurrentControlSet\Enum\USB\VID_3633&PID_0009";

    private static async Task<int> Main(string[] args)
    {
        string operation = args.Length == 0 ? "pipes" : args[0].ToLowerInvariant();
        if (operation is not ("pipes" or "status" or "init" or "info" or "stream" or "live"))
        {
            Console.Error.WriteLine("Usage: MystiqueUsbTool [pipes|status|init|info|stream [seconds]|live [updates]]");
            return 2;
        }

        try
        {
            string path = FindDevicePath();
            Console.WriteLine($"Device: {RedactSerial(path)}");

            using var file = Native.CreateFile(
                path,
                Native.GenericRead | Native.GenericWrite,
                Native.FileShareRead | Native.FileShareWrite,
                IntPtr.Zero,
                Native.OpenExisting,
                Native.FileAttributeNormal | Native.FileFlagOverlapped,
                IntPtr.Zero);

            if (file.IsInvalid)
                throw new Win32Exception(Marshal.GetLastWin32Error(),
                    "CreateFile failed. Close DeepCool before a protocol probe.");

            if (!Native.WinUsb_Initialize(file, out IntPtr usb))
                throw new Win32Exception(Marshal.GetLastWin32Error(), "WinUsb_Initialize failed");

            try
            {
                ListPipes(usb);
                if (operation == "status")
                {
                    byte[] response = Exchange(usb, 0x02, 0x82, 0x10, []);
                    PrintStatus(response);
                }
                else if (operation == "info")
                {
                    byte[] response = Exchange(usb, 0x01, 0x81, 0x12, []);
                    Console.WriteLine($"Info hex: {BitConverter.ToString(response).Replace('-', ' ')}");
                    Console.WriteLine($"Info text: {System.Text.Encoding.ASCII.GetString(response)
                        .Replace('\0', '.')}");
                }
                else if (operation == "init")
                {
                    InitializeMachineInfoMode(usb);
                }
                else if (operation == "stream")
                {
                    int seconds = args.Length >= 2 && int.TryParse(args[1], out int parsed)
                        ? Math.Clamp(parsed, 1, 600)
                        : 30;
                    StreamSyntheticTelemetry(usb, seconds);
                }
                else if (operation == "live")
                {
                    int updates = args.Length >= 2 && int.TryParse(args[1], out int parsed)
                        ? Math.Clamp(parsed, 0, 86_400)
                        : 0;
                    InitializeMachineInfoMode(usb);
                    await StreamLiveTelemetryAsync(usb, updates);
                }
            }
            finally
            {
                Native.WinUsb_Free(usb);
            }

            return 0;
        }
        catch (Exception ex)
        {
            string code = ex is Win32Exception win32 ? $" (Win32 {win32.NativeErrorCode})" : string.Empty;
            Console.Error.WriteLine($"ERROR{code}: {ex.Message}");
            return 1;
        }
    }

    private static string FindDevicePath()
    {
        using RegistryKey? product = Registry.LocalMachine.OpenSubKey(DeviceRegistryPath);
        if (product is null)
            throw new InvalidOperationException("MYSTIQUE VID 3633 / PID 0009 is not installed.");

        foreach (string serial in product.GetSubKeyNames())
        {
            using RegistryKey? parameters = product.OpenSubKey($@"{serial}\Device Parameters");
            string? symbolicName = parameters?.GetValue("SymbolicName") as string;
            if (string.IsNullOrWhiteSpace(symbolicName))
                continue;

            if (symbolicName.StartsWith(@"\??\", StringComparison.Ordinal))
                return @"\\?\" + symbolicName[4..];
            return symbolicName;
        }

        throw new InvalidOperationException("MYSTIQUE is registered, but has no WinUSB interface path.");
    }

    private static string RedactSerial(string path)
    {
        string[] parts = path.Split('#');
        return parts.Length >= 4 ? string.Join('#', parts[0], parts[1], "<serial>", parts[3]) : path;
    }

    private static void ListPipes(IntPtr usb)
    {
        if (!Native.WinUsb_QueryInterfaceSettings(usb, 0, out UsbInterfaceDescriptor descriptor))
            throw new Win32Exception(Marshal.GetLastWin32Error(), "WinUsb_QueryInterfaceSettings failed");

        Console.WriteLine($"Interface 0: {descriptor.NumEndpoints} endpoints");
        for (byte index = 0; index < descriptor.NumEndpoints; index++)
        {
            if (!Native.WinUsb_QueryPipe(usb, 0, index, out WinUsbPipeInformation pipe))
                throw new Win32Exception(Marshal.GetLastWin32Error(), $"WinUsb_QueryPipe({index}) failed");
            Console.WriteLine($"  0x{pipe.PipeId:X2} {pipe.PipeType,-9} max-packet={pipe.MaximumPacketSize}");
        }
    }

    private static void InitializeMachineInfoMode(IntPtr usb)
    {
        Console.WriteLine("Sending reversible Machine Info initialization...");
        var commands = new (byte Command, byte[] Payload)[]
        {
            (0x12, []),
            (0x02, [0x01, 0x00, 0x00, 0x01, 0x24]),
            (0x03, [0x01]),
            (0x04, [0x05, 0x00, 0x00, 0x01]),
            (0x07, [0x00, 0x02]),
            (0x08, [0x00, 0x04]),
            (0x05, [0x01, 0x01]),
            (0x0B, []),
            (0x06, [0x01]),
            (0x15, [0x2D, 0x2D]),
            (0x16, [0x2D, 0x2D]),
            (0x17, [0x2D, 0x2D]),
            (0x0A, [0xEA, 0x07, 0x02, 0x02, 0x02, 0x27, 0x21]),
        };

        foreach ((byte command, byte[] payload) in commands)
        {
            byte[] response = Exchange(usb, 0x01, 0x81, command, payload);
            Console.WriteLine($"  command 0x{command:X2}: {FormatPrefix(response)}");
            Thread.Sleep(command == 0x0A ? 100 : 10);
        }

        byte[] status = Exchange(usb, 0x02, 0x82, 0x10, []);
        PrintStatus(status);
        if (status.Length < 6 || status[5] != 0xFF)
            throw new InvalidOperationException("Device did not enter Machine Info mode.");
    }

    private static void StreamSyntheticTelemetry(IntPtr usb, int seconds)
    {
        Console.WriteLine($"Sending {seconds} one-per-second synthetic telemetry updates...");
        for (int second = 0; second < seconds; second++)
        {
            byte cpuTemp = (byte)(35 + second % 25);
            byte cpuUsage = (byte)(10 + second % 80);
            byte ramUsage = (byte)(40 + second % 20);
            byte gpuTemp = (byte)(45 + second % 20);
            ushort cpuMhz = (ushort)(4200 + (second % 8) * 100);
            byte[] payload = BuildDisplayPayload(cpuTemp, cpuUsage, ramUsage, gpuTemp, cpuMhz);
            byte[] response = Exchange(usb, 0x02, 0x82, 0x01, payload);
            Console.WriteLine(
                $"  {second + 1,3}/{seconds}: CPU {cpuTemp}C {cpuUsage}%  RAM {ramUsage}%  " +
                $"GPU {gpuTemp}C  {cpuMhz}MHz  reply {FormatPrefix(response)}");
            Thread.Sleep(1000);
        }
    }

    private static byte[] BuildDisplayPayload(
        byte cpuTemp, byte cpuUsage, byte ramUsage, byte gpuTemp, ushort cpuMhz)
    {
        byte[] payload = new byte[39];
        payload[0] = cpuTemp;
        payload[3] = cpuUsage;
        payload[6] = ramUsage;
        payload[8] = gpuTemp < 35 ? (byte)0x05 : gpuTemp < 38 ? (byte)0x06 : (byte)0x09;
        payload[9] = 0x03;
        payload[11] = gpuTemp;
        payload[12] = 0x05;
        payload[14] = 0x06;
        payload[15] = 0x0C;
        payload[17] = 0x07;
        payload[18] = (byte)(cpuMhz / 1000);
        payload[20] = (byte)Math.Max(0x0A, cpuMhz % 1000 / 10);
        ushort decimalDisplay = (ushort)((cpuMhz % 1000 / 10) * 100);
        payload[21] = (byte)(decimalDisplay & 0xFF);
        payload[22] = (byte)(decimalDisplay >> 8);
        payload[24] = payload[21];
        payload[25] = payload[22];
        return payload;
    }

    private static async Task StreamLiveTelemetryAsync(IntPtr usb, int updateLimit)
    {
        using var stop = new CancellationTokenSource();
        ConsoleCancelEventHandler cancelHandler = (_, e) =>
        {
            e.Cancel = true;
            stop.Cancel();
        };
        Console.CancelKeyPress += cancelHandler;

        try
        {
            using SensorBridgeClient sensors = await SensorBridgeClient.ConnectAsync(stop.Token);
            Console.WriteLine(updateLimit == 0
                ? "Streaming live telemetry until Ctrl+C..."
                : $"Streaming {updateLimit} live telemetry updates...");

            int sent = 0;
            while (!stop.IsCancellationRequested && (updateLimit == 0 || sent < updateLimit))
            {
                SensorSample sample = await sensors.ReadAsync(stop.Token);
                byte cpuTemp = ToByte(sample.CpuTemperature, 0, 127);
                byte cpuUsage = ToByte(sample.CpuUsage, 0, 100);
                byte memoryLoad = ToByte(sample.MemoryLoad, 0, 100);
                byte gpuTemp = ToByte(sample.GpuTemperature, 0, 127);
                ushort cpuClock = (ushort)Math.Clamp(
                    (int)Math.Round(sample.CpuClock), 0, ushort.MaxValue);

                byte[] payload = BuildDisplayPayload(
                    cpuTemp, cpuUsage, memoryLoad, gpuTemp, cpuClock);
                byte[] response = Exchange(usb, 0x02, 0x82, 0x01, payload);
                sent++;
                Console.WriteLine(
                    $"  {sent,5}: CPU {cpuTemp}C {cpuUsage}% {cpuClock}MHz  " +
                    $"RAM {memoryLoad}%  GPU {gpuTemp}C  reply {FormatPrefix(response)}");
            }
        }
        catch (OperationCanceledException) when (stop.IsCancellationRequested)
        {
            Console.WriteLine("Stopped.");
        }
        finally
        {
            Console.CancelKeyPress -= cancelHandler;
        }
    }

    private static byte ToByte(double value, int minimum, int maximum) =>
        (byte)Math.Clamp((int)Math.Round(value), minimum, maximum);

    private static byte[] Exchange(
        IntPtr usb, byte outPipe, byte inPipe, byte command, byte[] payload)
    {
        SetTimeout(usb, outPipe, 1500);
        SetTimeout(usb, inPipe, 1500);

        byte[] packet = BuildPacket(command, payload);
        if (!Native.WinUsb_WritePipe(usb, outPipe, packet, (uint)packet.Length,
                out uint written, IntPtr.Zero))
            throw PipeError("write", outPipe, command);
        if (written != packet.Length)
            throw new IOException($"Short write on 0x{outPipe:X2}: {written}/{packet.Length} bytes");

        byte[] response = new byte[64];
        if (!Native.WinUsb_ReadPipe(usb, inPipe, response, (uint)response.Length,
                out uint read, IntPtr.Zero))
            throw PipeError("read", inPipe, command);

        Array.Resize(ref response, checked((int)read));
        ValidateResponse(response, command);
        return response;
    }

    private static void SetTimeout(IntPtr usb, byte pipe, uint milliseconds)
    {
        uint value = milliseconds;
        if (!Native.WinUsb_SetPipePolicy(usb, pipe, Native.PipeTransferTimeout,
                sizeof(uint), ref value))
            throw new Win32Exception(Marshal.GetLastWin32Error(),
                $"Could not set timeout on pipe 0x{pipe:X2}");
    }

    private static Exception PipeError(string action, byte pipe, byte command)
    {
        int code = Marshal.GetLastWin32Error();
        return new Win32Exception(code,
            $"WinUSB {action} failed on pipe 0x{pipe:X2} for command 0x{command:X2} (Win32 {code})");
    }

    private static byte[] BuildPacket(byte command, byte[] payload)
    {
        if (payload.Length > 39)
            throw new ArgumentOutOfRangeException(nameof(payload));

        byte[] packet = new byte[48];
        packet[0] = 0xAA;
        packet[1] = 0x2E;
        packet[2] = command;
        payload.CopyTo(packet, 3);
        packet[42] = 0x48;
        packet[43] = 0x49;
        packet[44] = 0x44;
        packet[45] = 0x43;

        ushort checksum = 0;
        for (int i = 0; i < 46; i++)
            checksum += packet[i];
        packet[46] = (byte)(checksum & 0xFF);
        packet[47] = (byte)(checksum >> 8);
        return packet;
    }

    private static void ValidateResponse(byte[] response, byte command)
    {
        if (response.Length < 3)
            throw new InvalidDataException($"Short response to command 0x{command:X2}: {response.Length} bytes");
        if (response[0] != 0x55 || response[1] != 0x2E)
            throw new InvalidDataException($"Unexpected response header: {FormatPrefix(response)}");
        if (response[2] != command && response[2] != 0x00)
            throw new InvalidDataException(
                $"Response command mismatch: sent 0x{command:X2}, received 0x{response[2]:X2}");
    }

    private static void PrintStatus(byte[] response)
    {
        string mode = response.Length >= 6 ? response[5] switch
        {
            0xFF => "Machine Info",
            0x00 => "Image/GIF",
            byte value => $"unknown 0x{value:X2}",
        } : "missing";
        string rotation = response.Length >= 5 ? $"{response[4] * 90} degrees" : "missing";
        Console.WriteLine($"Status: mode={mode}, rotation={rotation}, response={FormatPrefix(response)}");
    }

    private static string FormatPrefix(byte[] bytes) =>
        BitConverter.ToString(bytes, 0, Math.Min(bytes.Length, 12)).Replace('-', ' ');
}

internal sealed record SensorSample(
    double CpuTemperature,
    double CpuUsage,
    double MemoryLoad,
    double GpuTemperature,
    double CpuClock);

internal sealed class SensorBridgeClient : IDisposable
{
    private const string ControlPipeName = "deepcool_sensor_data";
    private static readonly byte[] FrameMagic = [0xDE, 0xAD, 0xBE, 0xEF];
    private readonly NamedPipeClientStream control;
    private readonly NamedPipeClientStream data;

    private SensorBridgeClient(NamedPipeClientStream control, NamedPipeClientStream data)
    {
        this.control = control;
        this.data = data;
    }

    internal static async Task<SensorBridgeClient> ConnectAsync(CancellationToken cancellationToken)
    {
        var control = new NamedPipeClientStream(
            ".", ControlPipeName, PipeDirection.InOut, PipeOptions.Asynchronous);
        NamedPipeClientStream? data = null;
        try
        {
            await control.ConnectAsync(2_000, cancellationToken);
            control.ReadMode = PipeTransmissionMode.Message;

            string clientId = $"{DateTimeOffset.UtcNow.ToUnixTimeMilliseconds()}_mystique_live";
            await SendControlAsync(control,
                new { type = "SERVER_HELLO", seq = 0, payload = clientId }, cancellationToken);
            using JsonDocument hello = JsonDocument.Parse(
                await ReadControlAsync(control, cancellationToken));
            if (hello.RootElement.GetProperty("type").GetString() != "CLIENT_HELLO")
                throw new InvalidDataException("Sensor bridge rejected CLIENT_HELLO handshake.");

            await SendControlAsync(control,
                new { type = "SETUP_DATA_CHANNEL", seq = 1, payload = new { } }, cancellationToken);
            using JsonDocument ready = JsonDocument.Parse(
                await ReadControlAsync(control, cancellationToken));
            if (ready.RootElement.GetProperty("type").GetString() != "DATA_CHANNEL_READY")
                throw new InvalidDataException("Sensor bridge did not create a data channel.");

            string dataPath = ready.RootElement.GetProperty("payload")
                .GetProperty("DataPipeName").GetString()
                ?? throw new InvalidDataException("Sensor bridge omitted DataPipeName.");
            string dataPipeName = dataPath[(dataPath.LastIndexOf('\\') + 1)..];
            data = new NamedPipeClientStream(
                ".", dataPipeName, PipeDirection.In, PipeOptions.Asynchronous);
            await data.ConnectAsync(2_000, cancellationToken);

            await SendControlAsync(control,
                new { type = "GET_SENSOR_DATA", seq = 2, payload = new { } }, cancellationToken);
            return new SensorBridgeClient(control, data);
        }
        catch
        {
            data?.Dispose();
            control.Dispose();
            throw;
        }
    }

    internal async Task<SensorSample> ReadAsync(CancellationToken cancellationToken)
    {
        byte[] header = new byte[6];
        await data.ReadExactlyAsync(header.AsMemory(), cancellationToken);
        if (!header.AsSpan(0, 4).SequenceEqual(FrameMagic))
            throw new InvalidDataException(
                $"Unexpected sensor-frame magic: {Convert.ToHexString(header.AsSpan(0, 4))}");

        int jsonLength = BitConverter.ToUInt16(header, 4);
        byte[] jsonBytes = new byte[jsonLength];
        await data.ReadExactlyAsync(jsonBytes.AsMemory(), cancellationToken);
        byte[] trailer = new byte[2];
        await data.ReadExactlyAsync(trailer.AsMemory(), cancellationToken);

        using JsonDocument outer = JsonDocument.Parse(jsonBytes);
        if (outer.RootElement.GetProperty("type").GetString() != "GET_SENSOR_DATA")
            throw new InvalidDataException("Unexpected sensor-bridge data message.");
        string innerJson = outer.RootElement.GetProperty("payload").GetString()
            ?? throw new InvalidDataException("Sensor data payload was empty.");
        using JsonDocument inner = JsonDocument.Parse(innerJson);
        JsonElement root = inner.RootElement;

        return new SensorSample(
            ReadValue(root, "CPU Temperature"),
            ReadValue(root, "CPU Usage"),
            ReadValue(root, "Memory Load"),
            ReadValue(root, "Gpu temperature"),
            ReadValue(root, "CPU Clock"));
    }

    private static double ReadValue(JsonElement root, string name)
    {
        if (!root.TryGetProperty(name, out JsonElement sensor) ||
            !sensor.TryGetProperty("Value", out JsonElement value) ||
            !value.TryGetDouble(out double result))
            throw new InvalidDataException($"Sensor bridge omitted numeric '{name}'.");
        return result;
    }

    private static async Task SendControlAsync(
        PipeStream pipe, object message, CancellationToken cancellationToken)
    {
        byte[] bytes = Encoding.UTF8.GetBytes(JsonSerializer.Serialize(message));
        await pipe.WriteAsync(bytes.AsMemory(), cancellationToken);
        await pipe.FlushAsync(cancellationToken);
    }

    private static async Task<string> ReadControlAsync(
        PipeStream pipe, CancellationToken cancellationToken)
    {
        using var buffer = new MemoryStream();
        byte[] chunk = new byte[16 * 1024];
        do
        {
            int read = await pipe.ReadAsync(chunk.AsMemory(), cancellationToken);
            if (read == 0)
                throw new EndOfStreamException("Sensor control pipe closed during handshake.");
            buffer.Write(chunk, 0, read);
        } while (!pipe.IsMessageComplete);
        return Encoding.UTF8.GetString(buffer.ToArray());
    }

    public void Dispose()
    {
        data.Dispose();
        control.Dispose();
    }
}

internal enum UsbdPipeType
{
    Control,
    Isochronous,
    Bulk,
    Interrupt,
}

[StructLayout(LayoutKind.Sequential)]
internal struct UsbInterfaceDescriptor
{
    public byte Length;
    public byte DescriptorType;
    public byte InterfaceNumber;
    public byte AlternateSetting;
    public byte NumEndpoints;
    public byte InterfaceClass;
    public byte InterfaceSubClass;
    public byte InterfaceProtocol;
    public byte Interface;
}

[StructLayout(LayoutKind.Sequential)]
internal struct WinUsbPipeInformation
{
    public UsbdPipeType PipeType;
    public byte PipeId;
    public ushort MaximumPacketSize;
    public byte Interval;
}

internal static partial class Native
{
    internal const uint GenericRead = 0x80000000;
    internal const uint GenericWrite = 0x40000000;
    internal const uint FileShareRead = 0x00000001;
    internal const uint FileShareWrite = 0x00000002;
    internal const uint OpenExisting = 3;
    internal const uint FileAttributeNormal = 0x00000080;
    internal const uint FileFlagOverlapped = 0x40000000;
    internal const uint PipeTransferTimeout = 3;

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    internal static extern SafeFileHandle CreateFile(
        string fileName,
        uint desiredAccess,
        uint shareMode,
        IntPtr securityAttributes,
        uint creationDisposition,
        uint flagsAndAttributes,
        IntPtr templateFile);

    [DllImport("winusb.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool WinUsb_Initialize(SafeFileHandle deviceHandle, out IntPtr interfaceHandle);

    [DllImport("winusb.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool WinUsb_Free(IntPtr interfaceHandle);

    [DllImport("winusb.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool WinUsb_QueryInterfaceSettings(
        IntPtr interfaceHandle, byte alternateInterfaceNumber,
        out UsbInterfaceDescriptor usbAltInterfaceDescriptor);

    [DllImport("winusb.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool WinUsb_QueryPipe(
        IntPtr interfaceHandle, byte alternateInterfaceNumber, byte pipeIndex,
        out WinUsbPipeInformation pipeInformation);

    [DllImport("winusb.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool WinUsb_SetPipePolicy(
        IntPtr interfaceHandle, byte pipeId, uint policyType,
        uint valueLength, ref uint value);

    [DllImport("winusb.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool WinUsb_WritePipe(
        IntPtr interfaceHandle, byte pipeId, byte[] buffer, uint bufferLength,
        out uint lengthTransferred, IntPtr overlapped);

    [DllImport("winusb.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool WinUsb_ReadPipe(
        IntPtr interfaceHandle, byte pipeId, byte[] buffer, uint bufferLength,
        out uint lengthTransferred, IntPtr overlapped);
}
