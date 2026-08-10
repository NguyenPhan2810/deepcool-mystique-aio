using Mystique.Core.Contracts;
using Mystique.Core.Device;

namespace Mystique.Tests;

public class MystiquePacketsTests
{
    [Fact]
    public void PacketHasFixedFrameStructure()
    {
        byte[] packet = MystiquePackets.BuildPacket(0x12, []);

        Assert.Equal(48, packet.Length);
        Assert.Equal(0xAA, packet[0]);
        Assert.Equal(0x2E, packet[1]);
        Assert.Equal(0x12, packet[2]);
        Assert.Equal(new byte[] { 0x48, 0x49, 0x44, 0x43 }, packet[42..46]);
    }

    [Fact]
    public void ChecksumIsLittleEndianSumOfFirst46Bytes()
    {
        // 0xAA + 0x2E + 0x12 + 'H' + 'I' + 'D' + 'C' = 514 = 0x0202
        byte[] packet = MystiquePackets.BuildPacket(0x12, []);
        Assert.Equal(0x02, packet[46]);
        Assert.Equal(0x02, packet[47]);
    }

    [Fact]
    public void ChecksumAccountsForPayloadBytes()
    {
        // 514 - 0x12 + 0x01 + 55 = 552 = 0x0228
        byte[] packet = MystiquePackets.BuildPacket(0x01, [55]);
        Assert.Equal(0x28, packet[46]);
        Assert.Equal(0x02, packet[47]);

        int recomputed = 0;
        for (int i = 0; i < 46; i++)
            recomputed += packet[i];
        Assert.Equal(recomputed & 0xFFFF, packet[46] | (packet[47] << 8));
    }

    [Fact]
    public void PayloadLandsAtOffsetThree()
    {
        byte[] packet = MystiquePackets.BuildPacket(0x01, [0x11, 0x22, 0x33]);
        Assert.Equal(0x11, packet[3]);
        Assert.Equal(0x22, packet[4]);
        Assert.Equal(0x33, packet[5]);
        Assert.Equal(0x00, packet[6]);
    }

    [Fact]
    public void PayloadLongerThanThirtyNineBytesIsRejected()
    {
        Assert.Throws<ArgumentOutOfRangeException>(
            () => MystiquePackets.BuildPacket(0x01, new byte[40]));
    }

    [Fact]
    public void DisplayPayloadPlacesEveryFieldAtItsDocumentedOffset()
    {
        var frame = new TelemetryFrame(
            CpuTemperatureC: 55, CpuUsagePercent: 12, MemoryLoadPercent: 40,
            GpuTemperatureC: 48, CpuClockMhz: 4600);

        byte[] payload = MystiquePackets.BuildDisplayPayload(frame);

        Assert.Equal(39, payload.Length);
        Assert.Equal(55, payload[0]);    // CPU temperature
        Assert.Equal(12, payload[3]);    // CPU usage
        Assert.Equal(40, payload[6]);    // memory load
        Assert.Equal(0x09, payload[8]);  // GPU icon bucket for >= 38 C
        Assert.Equal(0x03, payload[9]);
        Assert.Equal(48, payload[11]);   // GPU temperature
        Assert.Equal(0x05, payload[12]);
        Assert.Equal(0x06, payload[14]);
        Assert.Equal(0x0C, payload[15]);
        Assert.Equal(0x07, payload[17]);
        Assert.Equal(4, payload[18]);    // GHz integer part
        Assert.Equal(60, payload[20]);   // hundredths, floored at 0x0A
        Assert.Equal(0x70, payload[21]); // 6000 low byte
        Assert.Equal(0x17, payload[22]); // 6000 high byte
        Assert.Equal(payload[21], payload[24]);
        Assert.Equal(payload[22], payload[25]);
    }

    [Theory]
    [InlineData((byte)30, (byte)0x05)]
    [InlineData((byte)36, (byte)0x06)]
    [InlineData((byte)48, (byte)0x09)]
    public void GpuIconBucketFollowsTemperatureThresholds(byte gpuTemp, byte expected)
    {
        var frame = new TelemetryFrame(50, 10, 40, gpuTemp, 4000);
        Assert.Equal(expected, MystiquePackets.BuildDisplayPayload(frame)[8]);
    }

    [Fact]
    public void ClockHundredthsNeverDropBelowTheFloor()
    {
        // 4005 MHz -> 4005 % 1000 / 10 == 0, which must be raised to 0x0A.
        var frame = new TelemetryFrame(50, 10, 40, 40, 4005);
        Assert.Equal(0x0A, MystiquePackets.BuildDisplayPayload(frame)[20]);
    }

    [Fact]
    public void InitSequenceHasThirteenCommandsInTheVerifiedOrder()
    {
        var sequence = MystiquePackets.BuildInitSequence(rotation: 0);

        Assert.Equal(13, sequence.Count);
        Assert.Equal(
            new byte[] { 0x12, 0x02, 0x03, 0x04, 0x07, 0x08, 0x05, 0x0B, 0x06, 0x15, 0x16, 0x17, 0x0A },
            sequence.Select(c => c.Command).ToArray());
    }

    [Theory]
    [InlineData((byte)0)]
    [InlineData((byte)1)]
    [InlineData((byte)2)]
    [InlineData((byte)3)]
    public void RotationIsPayloadByteTwoOfCommand02(byte rotation)
    {
        var command02 = MystiquePackets.BuildInitSequence(rotation).Single(c => c.Command == 0x02);

        Assert.Equal(new byte[] { 0x01, 0x00, rotation, 0x01, 0x24 }, command02.Payload);
    }

    [Fact]
    public void RotationAboveThreeIsRejected()
    {
        Assert.Throws<ArgumentOutOfRangeException>(() => MystiquePackets.BuildInitSequence(4));
    }

    [Fact]
    public void CommandZeroAGetsTheLongerSettleDelay()
    {
        var sequence = MystiquePackets.BuildInitSequence(0);
        Assert.Equal(100, sequence.Single(c => c.Command == 0x0A).SettleMilliseconds);
        Assert.All(sequence.Where(c => c.Command != 0x0A), c => Assert.Equal(10, c.SettleMilliseconds));
    }

    [Fact]
    public void ValidResponseIsAccepted()
    {
        MystiquePackets.ValidateResponse([0x55, 0x2E, 0x10, 0x00], 0x10);
        MystiquePackets.ValidateResponse([0x55, 0x2E, 0x00, 0x00], 0x10); // 0x00 is a wildcard ack
    }

    [Fact]
    public void ShortResponseIsRejected()
    {
        Assert.Throws<InvalidDataException>(() => MystiquePackets.ValidateResponse([0x55, 0x2E], 0x10));
    }

    [Fact]
    public void WrongResponseHeaderIsRejected()
    {
        Assert.Throws<InvalidDataException>(
            () => MystiquePackets.ValidateResponse([0xAA, 0x2E, 0x10], 0x10));
    }

    [Fact]
    public void MismatchedResponseCommandIsRejected()
    {
        Assert.Throws<InvalidDataException>(
            () => MystiquePackets.ValidateResponse([0x55, 0x2E, 0x11], 0x10));
    }

    [Fact]
    public void StatusParsesModeAndRotation()
    {
        // byte 4 = rotation index, byte 5 = mode
        var status = MystiquePackets.ParseStatus([0x55, 0x2E, 0x10, 0x00, 0x02, 0xFF]);

        Assert.Equal(DisplayMode.MachineInfo, status.Mode);
        Assert.Equal(2, status.Rotation);
        Assert.Equal(180, status.RotationDegrees);
        Assert.True(MystiquePackets.IsInMachineInfoMode([0x55, 0x2E, 0x10, 0x00, 0x02, 0xFF]));
    }

    [Fact]
    public void StatusRecognisesImageMode()
    {
        var status = MystiquePackets.ParseStatus([0x55, 0x2E, 0x10, 0x00, 0x00, 0x00]);
        Assert.Equal(DisplayMode.ImageOrGif, status.Mode);
    }

    [Fact]
    public void StatusTooShortToCarryModeIsUnknown()
    {
        var status = MystiquePackets.ParseStatus([0x55, 0x2E, 0x10]);
        Assert.Equal(DisplayMode.Unknown, status.Mode);
        Assert.False(MystiquePackets.IsInMachineInfoMode([0x55, 0x2E, 0x10]));
    }

    [Fact]
    public void FormatRedactsTheDeviceIdentifierByDefault()
    {
        // Real capture: bytes 4-11 are ASCII "AABBCCDD", a per-device identifier.
        byte[] response = [0x55, 0x2E, 0x12, 0x00, 0x41, 0x41, 0x42, 0x42, 0x43, 0x43, 0x44, 0x44];

        string formatted = MystiquePackets.FormatResponse(response);

        Assert.Equal("55 2E 12 00 .. .. .. .. .. .. .. ..", formatted);
        Assert.DoesNotContain("32 36", formatted);
    }

    [Fact]
    public void FormatCanShowTheDeviceIdentifierWhenExplicitlyRequested()
    {
        byte[] response = [0x55, 0x2E, 0x12, 0x00, 0x41, 0x41, 0x42, 0x42, 0x43, 0x43, 0x44, 0x44];

        string formatted = MystiquePackets.FormatResponse(response, showDeviceId: true);

        Assert.Equal("55 2E 12 00 41 41 42 42 43 43 44 44", formatted);
    }

    [Fact]
    public void FormatHandlesResponsesShorterThanTheRedactionWindow()
    {
        Assert.Equal("55 2E 10 00 ..", MystiquePackets.FormatResponse([0x55, 0x2E, 0x10, 0x00, 0x07]));
    }

    [Fact]
    public void AsciiFormattingAlsoRedactsTheDeviceIdentifier()
    {
        byte[] response = [0x55, 0x2E, 0x12, 0x00, 0x41, 0x41, 0x42, 0x42, 0x43, 0x43, 0x44, 0x44];

        string text = MystiquePackets.FormatResponseAscii(response);

        Assert.DoesNotContain("AABBCCDD", text);
        Assert.Equal("U..........", text[..11]);
    }

    [Fact]
    public void AsciiRedactionCoversTheWholeBodyNotJustTheFirstIdentifier()
    {
        // Live capture: a second stable 24-char hex identifier follows the short one.
        // Masking only bytes 4-11 leaked it.
        byte[] response = new byte[48];
        response[0] = 0x55;
        response[1] = 0x2E;
        response[2] = 0x12;
        "AABBCCDD00112233445566778899AABB"u8.ToArray().CopyTo(response, 4);
        "HIDC"u8.ToArray().CopyTo(response, 42);

        string text = MystiquePackets.FormatResponseAscii(response);

        Assert.DoesNotContain("AABBCCDD", text);
        Assert.DoesNotContain("00112233", text);
        Assert.DoesNotContain("8899AABB", text);
        Assert.Contains("HIDC", text); // the frame marker is structure, not identity
    }

    [Fact]
    public void AsciiFormattingCanShowTheDeviceIdentifier()
    {
        byte[] response = [0x55, 0x2E, 0x12, 0x00, 0x41, 0x41, 0x42, 0x42, 0x43, 0x43, 0x44, 0x44];

        Assert.Contains("AABBCCDD", MystiquePackets.FormatResponseAscii(response, showDeviceId: true));
    }
}
