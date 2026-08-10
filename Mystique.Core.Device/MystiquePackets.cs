using Mystique.Core.Contracts;

namespace Mystique.Core.Device;

public enum DisplayMode
{
    Unknown,
    MachineInfo,
    ImageOrGif,
}

/// <summary>One step of the Machine Info initialization handshake.</summary>
public readonly record struct InitCommand(byte Command, byte[] Payload, int SettleMilliseconds);

/// <summary>Decoded reply to status command 0x10.</summary>
public readonly record struct DeviceStatus(DisplayMode Mode, byte RawMode, byte Rotation)
{
    public int RotationDegrees => Rotation * 90;
}

/// <summary>
/// Pure wire-format layer: builds and parses packets, performs no I/O and never
/// writes to the console. Every constant here was confirmed against a live device.
/// </summary>
public static class MystiquePackets
{
    public const int PacketLength = 48;
    public const int MaxPayloadLength = 39;

    /// <summary>Endpoint pair used for initialization and info commands.</summary>
    public const byte InitPipeOut = 0x01;
    public const byte InitPipeIn = 0x81;

    /// <summary>Endpoint pair used for status and telemetry.</summary>
    public const byte DataPipeOut = 0x02;
    public const byte DataPipeIn = 0x82;

    public const byte CommandInfo = 0x12;
    public const byte CommandStatus = 0x10;
    public const byte CommandTelemetry = 0x01;

    /// <summary>
    /// The whole response body between the 4-byte header and the trailing "HIDC"
    /// marker is masked by default. Bytes 4-11 are the short device identifier
    /// (ASCII, e.g. "AABBCCDD"), but a live capture showed a second stable 24-char
    /// hex identifier immediately after it, so masking only the first field leaked
    /// the second. Redacting the body wholesale is the rule that cannot be
    /// out-flanked by another field appearing later.
    /// </summary>
    private const int DeviceIdOffset = 4;
    private const int DeviceIdLength = 38; // through byte 41, i.e. up to "HIDC" at 42
    private const int FormatPrefixLength = 12;

    public static byte[] BuildPacket(byte command, ReadOnlySpan<byte> payload)
    {
        if (payload.Length > MaxPayloadLength)
            throw new ArgumentOutOfRangeException(
                nameof(payload),
                payload.Length,
                $"Payload must be at most {MaxPayloadLength} bytes.");

        byte[] packet = new byte[PacketLength];
        packet[0] = 0xAA;
        packet[1] = 0x2E;
        packet[2] = command;
        payload.CopyTo(packet.AsSpan(3));
        packet[42] = 0x48; // 'H'
        packet[43] = 0x49; // 'I'
        packet[44] = 0x44; // 'D'
        packet[45] = 0x43; // 'C'

        ushort checksum = 0;
        for (int i = 0; i < 46; i++)
            checksum += packet[i];
        packet[46] = (byte)(checksum & 0xFF);
        packet[47] = (byte)(checksum >> 8);
        return packet;
    }

    public static byte[] BuildDisplayPayload(in TelemetryFrame frame)
    {
        byte[] payload = new byte[MaxPayloadLength];
        payload[0] = frame.CpuTemperatureC;
        payload[3] = frame.CpuUsagePercent;
        payload[6] = frame.MemoryLoadPercent;
        payload[8] = frame.GpuTemperatureC < 35 ? (byte)0x05
                   : frame.GpuTemperatureC < 38 ? (byte)0x06
                   : (byte)0x09;
        payload[9] = 0x03;
        payload[11] = frame.GpuTemperatureC;
        payload[12] = 0x05;
        payload[14] = 0x06;
        payload[15] = 0x0C;
        payload[17] = 0x07;
        payload[18] = (byte)(frame.CpuClockMhz / 1000);
        payload[20] = (byte)Math.Max(0x0A, frame.CpuClockMhz % 1000 / 10);

        ushort decimalDisplay = (ushort)(frame.CpuClockMhz % 1000 / 10 * 100);
        payload[21] = (byte)(decimalDisplay & 0xFF);
        payload[22] = (byte)(decimalDisplay >> 8);
        payload[24] = payload[21];
        payload[25] = payload[22];
        return payload;
    }

    /// <summary>
    /// The 13-command Machine Info handshake, in the order confirmed against the
    /// device. The order is not ascending by command byte; do not "tidy" it.
    /// </summary>
    public static IReadOnlyList<InitCommand> BuildInitSequence(byte rotation)
    {
        if (rotation > 3)
            throw new ArgumentOutOfRangeException(
                nameof(rotation), rotation, "Rotation must be 0-3 (0/90/180/270 degrees).");

        return
        [
            new(0x12, [], 10),
            new(0x02, [0x01, 0x00, rotation, 0x01, 0x24], 10),
            new(0x03, [0x01], 10),
            new(0x04, [0x05, 0x00, 0x00, 0x01], 10),
            new(0x07, [0x00, 0x02], 10),
            new(0x08, [0x00, 0x04], 10),
            new(0x05, [0x01, 0x01], 10),
            new(0x0B, [], 10),
            new(0x06, [0x01], 10),
            new(0x15, [0x2D, 0x2D], 10),
            new(0x16, [0x2D, 0x2D], 10),
            new(0x17, [0x2D, 0x2D], 10),
            new(0x0A, [0xEA, 0x07, 0x02, 0x02, 0x02, 0x27, 0x21], 100),
        ];
    }

    public static void ValidateResponse(ReadOnlySpan<byte> response, byte command)
    {
        if (response.Length < 3)
            throw new InvalidDataException(
                $"Short response to command 0x{command:X2}: {response.Length} bytes");
        if (response[0] != 0x55 || response[1] != 0x2E)
            throw new InvalidDataException(
                $"Unexpected response header: {FormatResponse(response)}");
        if (response[2] != command && response[2] != 0x00)
            throw new InvalidDataException(
                $"Response command mismatch: sent 0x{command:X2}, received 0x{response[2]:X2}");
    }

    public static DeviceStatus ParseStatus(ReadOnlySpan<byte> response)
    {
        byte rawMode = response.Length >= 6 ? response[5] : (byte)0;
        DisplayMode mode = response.Length < 6 ? DisplayMode.Unknown : rawMode switch
        {
            0xFF => DisplayMode.MachineInfo,
            0x00 => DisplayMode.ImageOrGif,
            _ => DisplayMode.Unknown,
        };
        byte rotation = response.Length >= 5 ? response[4] : (byte)0;
        return new DeviceStatus(mode, rawMode, rotation);
    }

    public static bool IsInMachineInfoMode(ReadOnlySpan<byte> statusResponse) =>
        ParseStatus(statusResponse).Mode == DisplayMode.MachineInfo;

    /// <summary>
    /// Hex-dumps the leading bytes of a response with the device identifier masked.
    /// Redaction is the default because the service logs unattended; the raw form
    /// exists only for an explicit, interactive opt-in.
    /// </summary>
    public static string FormatResponse(ReadOnlySpan<byte> response, bool showDeviceId = false)
    {
        int length = Math.Min(response.Length, FormatPrefixLength);
        var parts = new string[length];
        for (int i = 0; i < length; i++)
        {
            bool redact = !showDeviceId
                && i >= DeviceIdOffset
                && i < DeviceIdOffset + DeviceIdLength;
            parts[i] = redact ? ".." : response[i].ToString("X2");
        }

        return string.Join(' ', parts);
    }

    /// <summary>
    /// Renders a response as printable ASCII with the device identifier masked.
    /// The identifier is ASCII text in the response body, so the hex formatter
    /// alone is not sufficient redaction.
    /// </summary>
    public static string FormatResponseAscii(ReadOnlySpan<byte> response, bool showDeviceId = false)
    {
        var text = new char[response.Length];
        for (int i = 0; i < response.Length; i++)
        {
            bool redact = !showDeviceId
                && i >= DeviceIdOffset
                && i < DeviceIdOffset + DeviceIdLength;

            char c = (char)response[i];
            text[i] = redact || c < 0x20 || c > 0x7E ? '.' : c;
        }

        return new string(text);
    }
}
