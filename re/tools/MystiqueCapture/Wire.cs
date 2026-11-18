namespace MystiqueCapture;

/// <summary>Builds one 48-byte request packet, with knobs used only by the
/// checksum experiments. Everything else mirrors MystiquePackets.BuildPacket.</summary>
internal sealed record PacketSpec(
    byte Command,
    byte[] Payload,
    int ChecksumAdjust = 0,
    int ChecksumBytes = 46,
    bool Hidc = true,
    byte Sync = 0xAA);

internal static class Wire
{
    public const int PacketLength = 48;
    public const int MaxPayloadLength = 39;

    public static byte[] Build(PacketSpec spec)
    {
        if (spec.Payload.Length > MaxPayloadLength)
            throw new ArgumentOutOfRangeException(nameof(spec), "payload exceeds 39 bytes");

        byte[] packet = new byte[PacketLength];
        packet[0] = spec.Sync;
        packet[1] = 0x2E;
        packet[2] = spec.Command;
        spec.Payload.CopyTo(packet.AsSpan(3));

        if (spec.Hidc)
        {
            packet[42] = 0x48; // 'H'
            packet[43] = 0x49; // 'I'
            packet[44] = 0x44; // 'D'
            packet[45] = 0x43; // 'C'
        }

        ushort checksum = Sum(packet, spec.ChecksumBytes);
        checksum = unchecked((ushort)(checksum + spec.ChecksumAdjust));
        packet[46] = (byte)(checksum & 0xFF);
        packet[47] = (byte)(checksum >> 8);
        return packet;
    }

    public static ushort Sum(ReadOnlySpan<byte> data, int count)
    {
        int limit = Math.Min(count, data.Length);
        ushort checksum = 0;
        for (int i = 0; i < limit; i++)
            checksum = unchecked((ushort)(checksum + data[i]));
        return checksum;
    }

    public static string Hex(ReadOnlySpan<byte> data) =>
        string.Join(' ', data.ToArray().Select(b => b.ToString("X2")));

    public static string HexAt(ReadOnlySpan<byte> data, int offset, int length)
    {
        if (offset >= data.Length || length <= 0)
            return string.Empty;
        int end = Math.Min(data.Length, offset + length);
        return Hex(data[offset..end]);
    }

    public static string? HexByte(ReadOnlySpan<byte> data, int offset) =>
        offset < data.Length ? data[offset].ToString("X2") : null;
}
