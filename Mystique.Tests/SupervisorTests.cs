using Mystique.Core.Contracts;
using Mystique.Core.Device;
using MystiqueService;

namespace Mystique.Tests;

public class SupervisorTests
{
    private static readonly SupervisorOptions Options = new(Rotation: 0, SummaryEveryUpdates: 300);

    /// <summary>No real waiting: delays complete immediately so loops run at full speed.</summary>
    private static DelayAsync NoDelay => (_, _) => Task.CompletedTask;

    private static Supervisor Build(
        Func<IMystiqueDevice> deviceFactory,
        Func<CancellationToken, Task<ISensorSource>> sensorFactory,
        Func<bool> deepCoolRunning,
        ILog? log = null,
        SupervisorOptions? options = null,
        DelayAsync? delay = null)
    {
        var time = new FakeTimeProvider();
        return new Supervisor(
            deviceFactory,
            sensorFactory,
            new DeepCoolWatcher(deepCoolRunning, time),
            log ?? new ConsoleLog(),
            time,
            delay ?? NoDelay,
            options ?? Options);
    }

    private static readonly TimeSpan Deadline = TimeSpan.FromSeconds(10);

    /// <summary>
    /// Spins until the condition holds, failing loudly rather than hanging the run.
    /// </summary>
    private static async Task WaitFor(Func<bool> until, string what)
    {
        long start = Environment.TickCount64;
        while (!until())
        {
            if (Environment.TickCount64 - start > Deadline.TotalMilliseconds)
                Assert.Fail($"Timed out waiting for: {what}");
            await Task.Delay(1);
        }
    }

    /// <summary>
    /// Runs the supervisor until <paramref name="until"/> holds, then stops it.
    /// <paramref name="inspect"/> runs while the supervisor is still live - anything
    /// asserted after the stop would see the post-shutdown state (Stopped, device
    /// disposed), which is correct behaviour and tells you nothing.
    /// </summary>
    private static async Task RunUntil(
        Supervisor supervisor, Func<bool> until, string what = "condition", Action? inspect = null)
    {
        using var cts = new CancellationTokenSource();
        // Task.Run, not a bare call: RunAsync executes synchronously up to its first
        // real suspension, which would otherwise block this thread.
        Task run = Task.Run(() => supervisor.RunAsync(cts.Token));

        try
        {
            await WaitFor(until, what);
            inspect?.Invoke();
        }
        finally
        {
            await cts.CancelAsync();
            await run;
        }
    }

    [Fact]
    public async Task StreamsFramesOnTheHappyPath()
    {
        var device = new FakeMystiqueDevice();
        var sensors = new FakeSensorSource();
        Supervisor supervisor = Build(() => device, _ => Task.FromResult<ISensorSource>(sensors), () => false);

        await RunUntil(supervisor, () => device.TelemetryCalls >= 10);

        Assert.True(device.TelemetryCalls >= 10);
        Assert.Equal(1, device.InitCalls);
        Assert.Equal(ServiceState.Stopped, supervisor.Status.State);
    }

    [Fact]
    public async Task ReportsStreamingWhileFramesFlow()
    {
        var device = new FakeMystiqueDevice();
        var states = new List<ServiceState>();
        Supervisor supervisor = Build(
            () => device, _ => Task.FromResult<ISensorSource>(new FakeSensorSource()), () => false);
        supervisor.StatusChanged += s => states.Add(s.State);

        await RunUntil(supervisor, () => device.TelemetryCalls >= 5);

        Assert.Contains(ServiceState.Streaming, states);
    }

    [Fact]
    public async Task DeviceAbsentAtBootIsWaitingNotAnError()
    {
        int attempts = 0;
        var device = new FakeMystiqueDevice();
        var states = new List<ServiceState>();

        Supervisor supervisor = Build(
            () => ++attempts < 3
                ? throw new MystiqueUsbException(UsbFault.Classify(2), "CreateFile")
                : device,
            _ => Task.FromResult<ISensorSource>(new FakeSensorSource()),
            () => false);
        supervisor.StatusChanged += s => states.Add(s.State);

        await RunUntil(supervisor, () => device.TelemetryCalls >= 1);

        Assert.Contains(ServiceState.Waiting, states);
        Assert.DoesNotContain(ServiceState.Reconnecting, states);
        Assert.True(device.TelemetryCalls >= 1);
    }

    [Fact]
    public async Task AccessDeniedIsTreatedAsAYield()
    {
        var states = new List<ServiceState>();
        Supervisor supervisor = Build(
            () => throw new MystiqueUsbException(UsbFault.Classify(5), "CreateFile"),
            _ => Task.FromResult<ISensorSource>(new FakeSensorSource()),
            () => false);
        supervisor.StatusChanged += s => states.Add(s.State);

        await RunUntil(supervisor, () => states.Contains(ServiceState.Yielded));

        Assert.Contains(ServiceState.Yielded, states);
    }

    [Fact]
    public async Task DeepCoolAppearingReleasesTheHandleWithoutTouchingTheDevice()
    {
        var device = new FakeMystiqueDevice();
        bool deepCool = false;
        Supervisor supervisor = Build(
            () => device, _ => Task.FromResult<ISensorSource>(new FakeSensorSource()), () => deepCool);

        using var cts = new CancellationTokenSource();
        Task run = Task.Run(() => supervisor.RunAsync(cts.Token));
        await WaitFor(() => device.TelemetryCalls >= 3, "three frames");

        deepCool = true;
        await WaitFor(() => device.Disposed, "handle released");
        int callsAtYield = device.TelemetryCalls;

        await Task.Delay(200);

        // Captured while still running: after the stop everything reads Stopped.
        int callsAfterWaiting = device.TelemetryCalls;
        ServiceState stateWhileYielded = supervisor.Status.State;

        await cts.CancelAsync();
        await run;

        Assert.Equal(callsAtYield, callsAfterWaiting); // zero device calls while yielded
        Assert.Equal(ServiceState.Yielded, stateWhileYielded);
    }

    [Fact]
    public async Task DeepCoolExitingReinitialisesTheDeviceNotJustReopensIt()
    {
        var devices = new List<FakeMystiqueDevice>();
        bool deepCool = true;
        Supervisor supervisor = Build(
            () => { var d = new FakeMystiqueDevice(); devices.Add(d); return d; },
            _ => Task.FromResult<ISensorSource>(new FakeSensorSource()),
            () => deepCool);

        using var cts = new CancellationTokenSource();
        Task run = Task.Run(() => supervisor.RunAsync(cts.Token));
        await WaitFor(() => supervisor.Status.State == ServiceState.Yielded, "yielded");

        deepCool = false;
        await WaitFor(() => devices.Count > 0 && devices[^1].TelemetryCalls >= 1, "reacquired");

        await cts.CancelAsync();
        await run;

        // A reopened device is not in Machine Info mode. Init must re-run, or
        // telemetry would be ACKed while the panel showed nothing.
        Assert.Equal(1, devices[^1].InitCalls);
    }

    [Fact]
    public async Task SurpriseRemovalDisposesTheDeviceAndReacquires()
    {
        var devices = new List<FakeMystiqueDevice>();
        Supervisor supervisor = Build(
            () =>
            {
                var d = new FakeMystiqueDevice();
                if (devices.Count == 0)
                    d.TelemetryFault = n => n == 3
                        ? new MystiqueUsbException(UsbFault.Classify(1167), "write")
                        : null;
                devices.Add(d);
                return d;
            },
            _ => Task.FromResult<ISensorSource>(new FakeSensorSource()),
            () => false);

        await RunUntil(supervisor, () => devices.Count >= 2 && devices[1].TelemetryCalls >= 2);

        Assert.True(devices[0].Disposed);
        Assert.Equal(1, devices[1].InitCalls);
    }

    [Fact]
    public async Task SensorFailureDegradesButKeepsTheDevice()
    {
        var device = new FakeMystiqueDevice();
        int connects = 0;
        var states = new List<ServiceState>();

        Supervisor supervisor = Build(
            () => device,
            _ => ++connects == 1
                ? Task.FromException<ISensorSource>(new TimeoutException("bridge not up"))
                : Task.FromResult<ISensorSource>(new FakeSensorSource()),
            () => false);
        supervisor.StatusChanged += s => states.Add(s.State);

        bool disposedWhileRunning = true;
        await RunUntil(
            supervisor,
            () => device.TelemetryCalls >= 3,
            "three frames after sensor recovery",
            inspect: () => disposedWhileRunning = device.Disposed);

        Assert.Contains(ServiceState.Degraded, states);
        Assert.False(disposedWhileRunning); // a sensor outage must not cost us the handle
    }

    [Fact]
    public async Task AnUnknownExceptionNeverEscapesTheLoop()
    {
        int attempts = 0;
        var device = new FakeMystiqueDevice();
        Supervisor supervisor = Build(
            () => ++attempts < 4 ? throw new InvalidOperationException("nonsense") : device,
            _ => Task.FromResult<ISensorSource>(new FakeSensorSource()),
            () => false);

        await RunUntil(supervisor, () => device.TelemetryCalls >= 1);

        Assert.True(device.TelemetryCalls >= 1); // recovered rather than dying
    }

    [Fact]
    public async Task CancellationStopsCleanlyAndReleasesTheDevice()
    {
        var device = new FakeMystiqueDevice();
        var sensors = new FakeSensorSource();
        Supervisor supervisor = Build(() => device, _ => Task.FromResult<ISensorSource>(sensors), () => false);

        using var cts = new CancellationTokenSource();
        Task run = Task.Run(() => supervisor.RunAsync(cts.Token));
        await WaitFor(() => device.TelemetryCalls >= 2, "two frames");
        await cts.CancelAsync();
        await run;

        Assert.True(device.Disposed);
        Assert.True(sensors.Disposed);
        Assert.Equal(ServiceState.Stopped, supervisor.Status.State);
        Assert.True(device.AbortCalls >= 1);
    }

    [Fact]
    public async Task RotationIsAppliedOnEveryInitSoItSurvivesReconnects()
    {
        var devices = new List<FakeMystiqueDevice>();
        Supervisor supervisor = Build(
            () =>
            {
                var d = new FakeMystiqueDevice();
                if (devices.Count == 0)
                    d.TelemetryFault = n => n == 2
                        ? new MystiqueUsbException(UsbFault.Classify(1167), "write")
                        : null;
                devices.Add(d);
                return d;
            },
            _ => Task.FromResult<ISensorSource>(new FakeSensorSource()),
            () => false,
            options: new SupervisorOptions(Rotation: 2));

        await RunUntil(supervisor, () => devices.Count >= 2 && devices[1].TelemetryCalls >= 1);

        Assert.All(devices, d => Assert.Equal([(byte)2], d.RotationsRequested));
    }

    [Fact]
    public async Task SetRotationForcesReinitialisation()
    {
        var device = new FakeMystiqueDevice();
        Supervisor supervisor = Build(
            () => device, _ => Task.FromResult<ISensorSource>(new FakeSensorSource()), () => false);

        using var cts = new CancellationTokenSource();
        Task run = Task.Run(() => supervisor.RunAsync(cts.Token));
        await WaitFor(() => device.TelemetryCalls >= 2, "two frames");

        supervisor.SetRotation(3);
        await WaitFor(() => device.InitCalls >= 2, "reinitialised");

        await cts.CancelAsync();
        await run;

        Assert.Equal([(byte)0, (byte)3], device.RotationsRequested);
    }

    [Fact]
    public async Task StatusCarriesTheLastFrameAndUpdateCount()
    {
        var device = new FakeMystiqueDevice();
        Supervisor supervisor = Build(
            () => device, _ => Task.FromResult<ISensorSource>(new FakeSensorSource()), () => false);

        await RunUntil(supervisor, () => device.TelemetryCalls >= 4);

        Assert.True(supervisor.Status.UpdatesSent >= 4);
        Assert.NotNull(supervisor.Status.Last);
        Assert.Equal(55, supervisor.Status.Last!.Value.CpuTemperatureC);
    }

    [Fact]
    public async Task APermanentlyAbsentDeviceStopsBeingProbedEverySecond()
    {
        var delays = new List<TimeSpan>();
        DelayAsync recording = (d, _) => { delays.Add(d); return Task.CompletedTask; };

        Supervisor supervisor = Build(
            () => throw new MystiqueUsbException(UsbFault.Classify(2), "CreateFile"),
            _ => Task.FromResult<ISensorSource>(new FakeSensorSource()),
            () => false,
            delay: recording);

        await RunUntil(supervisor, () => delays.Count >= 60);

        // The first 30 attempts stay on the fast schedule so a boot-time arrival is
        // picked up promptly; after that it must back off rather than probe forever.
        Assert.All(delays.Take(30), d => Assert.InRange(d.TotalMilliseconds, 800, 1200));
        Assert.Contains(delays.Skip(30), d => d.TotalSeconds > 3);
    }

    [Fact]
    public async Task PerUpdateLoggingIsNotEmitted()
    {
        var device = new FakeMystiqueDevice();
        var log = new ConsoleLog();
        Supervisor supervisor = Build(
            () => device, _ => Task.FromResult<ISensorSource>(new FakeSensorSource()), () => false,
            log: log, options: new SupervisorOptions(Rotation: 0, SummaryEveryUpdates: 300));

        int lines = 0;
        int frames = 0;
        await RunUntil(
            supervisor,
            () => device.TelemetryCalls >= 100,
            "one hundred frames",
            inspect: () =>
            {
                lines = log.Recent(int.MaxValue).Count;
                frames = device.TelemetryCalls;
            });

        // The count that matters is lines per frame, not lines outright: the fake
        // device completes instantly, so this loop runs thousands of frames in the
        // time the poller takes to notice 100. One summary per 300 frames plus a
        // couple of startup lines is the contract; one line per frame is the defect.
        Assert.True(
            lines <= frames / 300 + 5,
            $"expected about one line per 300 frames, got {lines} lines for {frames} frames");
        Assert.True(frames >= 100);
    }
}
