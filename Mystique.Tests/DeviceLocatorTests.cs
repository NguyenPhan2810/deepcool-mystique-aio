using Mystique.Core.Device;

namespace Mystique.Tests;

public class DeviceLocatorTests
{
    private const string RealPath =
        @"\\?\USB#VID_3633&PID_0009#0011223344556677889900AA#{a5dcbf10-6530-11d2-901f-00c04fb951ed}";

    private static char[] MultiSz(params string[] entries)
    {
        var chars = new List<char>();
        foreach (string entry in entries)
        {
            chars.AddRange(entry);
            chars.Add('\0');
        }

        chars.Add('\0'); // list terminator
        return chars.ToArray();
    }

    [Fact]
    public void ParsesADoubleNullTerminatedList()
    {
        var entries = DeviceLocator.ParseInterfaceList(MultiSz("alpha", "beta", "gamma"));

        Assert.Equal(new[] { "alpha", "beta", "gamma" }, entries);
    }

    [Fact]
    public void ParsesAnEmptyList()
    {
        Assert.Empty(DeviceLocator.ParseInterfaceList(new[] { '\0' }));
        Assert.Empty(DeviceLocator.ParseInterfaceList(ReadOnlySpan<char>.Empty));
    }

    [Fact]
    public void ParsesASingleEntry()
    {
        Assert.Equal(new[] { RealPath }, DeviceLocator.ParseInterfaceList(MultiSz(RealPath)));
    }

    [Fact]
    public void RecognisesTheMystiqueHardwareId()
    {
        Assert.True(DeviceLocator.IsMystiqueInterface(RealPath));
    }

    [Fact]
    public void MatchingIsCaseInsensitiveBecauseWindowsVariesTheCasing()
    {
        Assert.True(DeviceLocator.IsMystiqueInterface(RealPath.ToUpperInvariant()));
        Assert.True(DeviceLocator.IsMystiqueInterface(RealPath.ToLowerInvariant()));
    }

    [Theory]
    [InlineData(@"\\?\USB#VID_1038&PID_1122#abc#{a5dcbf10-6530-11d2-901f-00c04fb951ed}")]
    [InlineData(@"\\?\USB#VID_3633&PID_0008#abc#{a5dcbf10-6530-11d2-901f-00c04fb951ed}")]
    [InlineData("")]
    public void RejectsOtherDevices(string path)
    {
        Assert.False(DeviceLocator.IsMystiqueInterface(path));
    }

    [Fact]
    public void RedactionRemovesTheSerialButKeepsTheShape()
    {
        string redacted = DeviceLocator.RedactSerial(RealPath);

        Assert.DoesNotContain("0011223344556677889900AA", redacted);
        Assert.Contains("VID_3633&PID_0009", redacted);
        Assert.Contains("<serial>", redacted);
    }

    [Fact]
    public void RedactionLeavesUnexpectedShapesAlone()
    {
        Assert.Equal("not-a-device-path", DeviceLocator.RedactSerial("not-a-device-path"));
    }

    [Fact]
    [Trait("Category", "Hardware")]
    public void FindsThePresentDeviceOnThisMachine()
    {
        string? path = DeviceLocator.FindPresentDevicePath();

        Assert.NotNull(path);
        Assert.Contains(DeviceLocator.HardwareIdFragment, path, StringComparison.OrdinalIgnoreCase);
        Assert.StartsWith(@"\\?\", path, StringComparison.Ordinal);
    }
}
