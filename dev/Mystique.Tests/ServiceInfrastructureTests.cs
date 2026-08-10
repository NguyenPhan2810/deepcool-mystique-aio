using System.Security.AccessControl;
using System.Security.Principal;
using Mystique.Core.Device;
using MystiqueService;

namespace Mystique.Tests;

public sealed class RollingFileLogTests : IDisposable
{
    private readonly string directory = Path.Combine(
        Path.GetTempPath(), "mystique-log-tests", Guid.NewGuid().ToString("N"));

    public void Dispose()
    {
        if (Directory.Exists(directory))
            Directory.Delete(directory, recursive: true);
    }

    [Fact]
    public void EveryLineCarriesATimestampAndLevel()
    {
        var time = new FakeTimeProvider();
        using (var log = new RollingFileLog(directory, time))
            log.Write(LogLevel.Warn, "device not present");

        string line = File.ReadAllLines(Path.Combine(directory, "mystique.log")).Single();

        Assert.StartsWith("2026-01-01T00:00:00", line, StringComparison.Ordinal);
        Assert.Contains("WARN", line);
        Assert.EndsWith("device not present", line, StringComparison.Ordinal);
    }

    [Fact]
    public void TimestampsAdvanceWithTheClock()
    {
        var time = new FakeTimeProvider();
        using (var log = new RollingFileLog(directory, time))
        {
            log.Write(LogLevel.Info, "first");
            time.Advance(TimeSpan.FromMinutes(90));
            log.Write(LogLevel.Info, "second");
        }

        string[] lines = File.ReadAllLines(Path.Combine(directory, "mystique.log"));

        Assert.StartsWith("2026-01-01T00:00:00", lines[0], StringComparison.Ordinal);
        Assert.StartsWith("2026-01-01T01:30:00", lines[1], StringComparison.Ordinal);
    }

    [Fact]
    public void RotatesOnceTheSizeCapIsExceeded()
    {
        var time = new FakeTimeProvider();
        using (var log = new RollingFileLog(directory, time, maxBytes: 512, generations: 3))
        {
            for (int i = 0; i < 100; i++)
                log.Write(LogLevel.Info, $"line {i} padded out to force rotation quickly");
        }

        Assert.True(File.Exists(Path.Combine(directory, "mystique.log")));
        Assert.True(File.Exists(Path.Combine(directory, "mystique.1.log")));
    }

    [Fact]
    public void NeverKeepsMoreThanTheConfiguredGenerations()
    {
        var time = new FakeTimeProvider();
        using (var log = new RollingFileLog(directory, time, maxBytes: 256, generations: 2))
        {
            for (int i = 0; i < 400; i++)
                log.Write(LogLevel.Info, $"line {i} padded out to force many rotations");
        }

        Assert.Equal(2, Directory.GetFiles(directory, "mystique*.log").Length);
    }

    [Fact]
    public void RecentReturnsTheNewestLinesWithoutReadingTheFile()
    {
        var time = new FakeTimeProvider();
        using var log = new RollingFileLog(directory, time);

        for (int i = 0; i < 500; i++)
            log.Write(LogLevel.Info, $"line {i}");

        var recent = log.Recent(10);

        Assert.Equal(10, recent.Count);
        Assert.EndsWith("line 499", recent[^1], StringComparison.Ordinal);
        Assert.EndsWith("line 490", recent[0], StringComparison.Ordinal);
    }

    [Fact]
    public void RecentIsBoundedEvenWhenAskedForMore()
    {
        var time = new FakeTimeProvider();
        using var log = new RollingFileLog(directory, time);
        log.Write(LogLevel.Info, "only one");

        Assert.Single(log.Recent(1000));
    }

    [Fact]
    public void ConcurrentWritersDoNotCorruptOrLoseLines()
    {
        var time = new FakeTimeProvider();
        using (var log = new RollingFileLog(directory, time))
        {
            Parallel.For(0, 200, i => log.Write(LogLevel.Info, $"line {i}"));
        }

        string[] lines = File.ReadAllLines(Path.Combine(directory, "mystique.log"));

        Assert.Equal(200, lines.Length);
        Assert.All(lines, l => Assert.Contains("INFO", l));
    }

    [Fact]
    public void SecureDirectoryDisablesInheritanceAndDeniesUserWrites()
    {
        RollingFileLog.EnsureSecureDirectory(directory);

        DirectorySecurity security = new DirectoryInfo(directory).GetAccessControl();
        AuthorizationRuleCollection rules =
            security.GetAccessRules(true, true, typeof(SecurityIdentifier));

        Assert.True(security.AreAccessRulesProtected);

        var users = new SecurityIdentifier(WellKnownSidType.BuiltinUsersSid, null);
        foreach (FileSystemAccessRule rule in rules)
        {
            if (!rule.IdentityReference.Equals(users))
                continue;

            Assert.Equal(AccessControlType.Allow, rule.AccessControlType);
            Assert.Equal(0, (int)(rule.FileSystemRights & FileSystemRights.Write));
        }
    }

    [Fact]
    public void EnsureSecureDirectoryIsIdempotent()
    {
        RollingFileLog.EnsureSecureDirectory(directory);
        RollingFileLog.EnsureSecureDirectory(directory);

        Assert.True(Directory.Exists(directory));
    }

    [Fact]
    public void CompositeWritesToEverySink()
    {
        var time = new FakeTimeProvider();
        using var first = new RollingFileLog(Path.Combine(directory, "a"), time);
        using var second = new RollingFileLog(Path.Combine(directory, "b"), time);
        var composite = new CompositeLog(first, second);

        composite.Write(LogLevel.Error, "both");

        Assert.Single(first.Recent(10));
        Assert.Single(second.Recent(10));
    }
}

public class DeepCoolWatcherTests
{
    [Fact]
    public void ReportsNoTransitionWhileDeepCoolStaysAway()
    {
        bool running = false;
        var watcher = new DeepCoolWatcher(() => running, new FakeTimeProvider());

        Assert.Equal(DeepCoolTransition.Unchanged, watcher.Poll());
        Assert.Equal(DeepCoolTransition.Unchanged, watcher.Poll());
        Assert.False(watcher.IsPresent);
    }

    [Fact]
    public void DetectsAppearanceExactlyOnce()
    {
        bool running = false;
        var watcher = new DeepCoolWatcher(() => running, new FakeTimeProvider());
        watcher.Poll();

        running = true;

        Assert.Equal(DeepCoolTransition.Appeared, watcher.Poll());
        Assert.Equal(DeepCoolTransition.Unchanged, watcher.Poll());
        Assert.True(watcher.IsPresent);
    }

    [Fact]
    public void DetectsExitExactlyOnce()
    {
        bool running = true;
        var watcher = new DeepCoolWatcher(() => running, new FakeTimeProvider());
        watcher.Poll();

        running = false;

        Assert.Equal(DeepCoolTransition.Exited, watcher.Poll());
        Assert.Equal(DeepCoolTransition.Unchanged, watcher.Poll());
        Assert.False(watcher.IsPresent);
    }

    [Fact]
    public void DeepCoolAlreadyRunningAtStartupCountsAsAnAppearance()
    {
        var watcher = new DeepCoolWatcher(() => true, new FakeTimeProvider());

        Assert.Equal(DeepCoolTransition.Appeared, watcher.Poll());
    }

    [Fact]
    public void HeartbeatIsSilentForTheFirstFifteenMinutes()
    {
        var time = new FakeTimeProvider();
        var watcher = new DeepCoolWatcher(() => true, time);
        watcher.Poll();

        time.Advance(TimeSpan.FromMinutes(14));

        Assert.False(watcher.ShouldLogHeartbeat());
    }

    [Fact]
    public void HeartbeatFiresEveryFifteenMinutesWhileYielded()
    {
        var time = new FakeTimeProvider();
        var watcher = new DeepCoolWatcher(() => true, time);
        watcher.Poll();

        time.Advance(TimeSpan.FromMinutes(15));
        Assert.True(watcher.ShouldLogHeartbeat());
        Assert.False(watcher.ShouldLogHeartbeat());

        time.Advance(TimeSpan.FromMinutes(15));
        Assert.True(watcher.ShouldLogHeartbeat());
    }

    [Fact]
    public void EightHoursYieldedProducesOneHeartbeatPerQuarterHour()
    {
        var time = new FakeTimeProvider();
        var watcher = new DeepCoolWatcher(() => true, time);
        watcher.Poll();
        int beats = 0;

        // Poll every 2 s for 8 hours.
        for (int i = 0; i < 8 * 1800; i++)
        {
            watcher.Poll();
            if (watcher.ShouldLogHeartbeat())
                beats++;
            time.Advance(TimeSpan.FromSeconds(2));
        }

        // 480 minutes / 15 = 32 intervals, but the last one has not fully elapsed
        // when the loop ends, so 31 is the correct count.
        Assert.Equal(31, beats);
    }

    [Fact]
    public void HeartbeatDoesNotFireWhenDeepCoolIsAbsent()
    {
        var time = new FakeTimeProvider();
        var watcher = new DeepCoolWatcher(() => false, time);
        watcher.Poll();

        time.Advance(TimeSpan.FromHours(4));

        Assert.False(watcher.ShouldLogHeartbeat());
    }

    [Fact]
    public void HeartbeatClockRestartsOnEachAppearance()
    {
        var time = new FakeTimeProvider();
        bool running = true;
        var watcher = new DeepCoolWatcher(() => running, time);
        watcher.Poll();

        time.Advance(TimeSpan.FromMinutes(14));
        running = false;
        watcher.Poll();
        running = true;
        watcher.Poll();

        time.Advance(TimeSpan.FromMinutes(14));
        Assert.False(watcher.ShouldLogHeartbeat());

        time.Advance(TimeSpan.FromMinutes(1));
        Assert.True(watcher.ShouldLogHeartbeat());
    }

    [Fact]
    [Trait("Category", "Hardware")]
    public void RealProcessProbeAgreesWithTheProcessTable()
    {
        bool expected = System.Diagnostics.Process.GetProcessesByName("DeepCool").Length > 0;

        Assert.Equal(expected, DeepCoolWatcher.ProcessIsRunning());
    }
}

public sealed class ServiceConfigTests : IDisposable
{
    private readonly string path = Path.Combine(
        Path.GetTempPath(), $"mystique-config-{Guid.NewGuid():N}.json");

    public void Dispose()
    {
        if (File.Exists(path))
            File.Delete(path);
    }

    [Fact]
    public void MissingFileYieldsDefaults() => Assert.Equal(0, ServiceConfig.Load(path).Rotation);

    [Fact]
    public void RoundTripsThroughDisk()
    {
        new ServiceConfig(Rotation: 3).Save(path);

        Assert.Equal(3, ServiceConfig.Load(path).Rotation);
    }

    [Fact]
    public void CorruptFileFallsBackToDefaultsRatherThanThrowing()
    {
        File.WriteAllText(path, "{ this is not json");

        Assert.Equal(0, ServiceConfig.Load(path).Rotation);
    }

    [Fact]
    public void OutOfRangeRotationIsClampedOnLoad()
    {
        File.WriteAllText(path, """{ "Rotation": 9 }""");

        Assert.Equal(0, ServiceConfig.Load(path).Rotation);
    }

    [Fact]
    public void SaveCreatesTheParentDirectory()
    {
        string nested = Path.Combine(
            Path.GetTempPath(), $"mystique-cfg-{Guid.NewGuid():N}", "config.json");
        try
        {
            new ServiceConfig(1).Save(nested);
            Assert.True(File.Exists(nested));
        }
        finally
        {
            Directory.Delete(Path.GetDirectoryName(nested)!, recursive: true);
        }
    }
}
