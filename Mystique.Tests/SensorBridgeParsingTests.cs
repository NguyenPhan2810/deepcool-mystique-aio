using System.Text;
using System.Text.Json;
using Mystique.Core.Device;

namespace Mystique.Tests;

public class SensorBridgeParsingTests
{
    private static byte[] Envelope(string innerJson) =>
        Encoding.UTF8.GetBytes(JsonSerializer.Serialize(
            new { type = "GET_SENSOR_DATA", payload = innerJson }));

    private const string GoodInner = """
        {
          "CPU Temperature": { "Value": 55.5 },
          "CPU Usage":       { "Value": 12.25 },
          "Memory Load":     { "Value": 41.0 },
          "Gpu temperature": { "Value": 48.0 },
          "CPU Clock":       { "Value": 4612.5 }
        }
        """;

    [Fact]
    public void ReadsEverySensorValue()
    {
        SensorSample sample = SensorBridgeClient.ParseSensorPayload(Envelope(GoodInner));

        Assert.Equal(55.5, sample.CpuTemperature);
        Assert.Equal(12.25, sample.CpuUsage);
        Assert.Equal(41.0, sample.MemoryLoad);
        Assert.Equal(48.0, sample.GpuTemperature);
        Assert.Equal(4612.5, sample.CpuClock);
    }

    [Fact]
    public void RejectsAnUnexpectedMessageType()
    {
        byte[] bytes = Encoding.UTF8.GetBytes(JsonSerializer.Serialize(
            new { type = "CLIENT_HELLO", payload = GoodInner }));

        Assert.Throws<InvalidDataException>(() => SensorBridgeClient.ParseSensorPayload(bytes));
    }

    [Fact]
    public void RejectsAMissingSensor()
    {
        string missingGpu = """
            {
              "CPU Temperature": { "Value": 55.5 },
              "CPU Usage":       { "Value": 12.25 },
              "Memory Load":     { "Value": 41.0 },
              "CPU Clock":       { "Value": 4612.5 }
            }
            """;

        var error = Assert.Throws<InvalidDataException>(
            () => SensorBridgeClient.ParseSensorPayload(Envelope(missingGpu)));
        Assert.Contains("Gpu temperature", error.Message);
    }

    [Fact]
    public void RejectsANonNumericSensorValue()
    {
        string textValue = """
            {
              "CPU Temperature": { "Value": "hot" },
              "CPU Usage":       { "Value": 12.25 },
              "Memory Load":     { "Value": 41.0 },
              "Gpu temperature": { "Value": 48.0 },
              "CPU Clock":       { "Value": 4612.5 }
            }
            """;

        Assert.Throws<InvalidDataException>(
            () => SensorBridgeClient.ParseSensorPayload(Envelope(textValue)));
    }

    [Fact]
    public void ReadsTheFrameLengthFromAValidHeader()
    {
        // DE AD BE EF then a little-endian ushort length.
        byte[] header = [0xDE, 0xAD, 0xBE, 0xEF, 0x34, 0x12];

        Assert.Equal(0x1234, SensorBridgeClient.ReadFrameLength(header));
    }

    [Fact]
    public void RejectsAFrameWithWrongMagic()
    {
        byte[] header = [0xDE, 0xAD, 0xBE, 0xEE, 0x34, 0x12];

        var error = Assert.Throws<InvalidDataException>(
            () => SensorBridgeClient.ReadFrameLength(header));
        Assert.Contains("magic", error.Message, StringComparison.OrdinalIgnoreCase);
    }

    [Fact]
    public void RejectsATruncatedHeader()
    {
        Assert.Throws<InvalidDataException>(
            () => SensorBridgeClient.ReadFrameLength([0xDE, 0xAD, 0xBE]));
    }
}
