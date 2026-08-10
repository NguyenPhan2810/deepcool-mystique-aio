namespace Mystique.Tests;

public class ScaffoldingTests
{
    [Fact]
    public void TestProjectTargetsWindowsTfm()
    {
        Assert.True(OperatingSystem.IsWindows());
    }
}
