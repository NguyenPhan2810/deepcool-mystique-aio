using Mystique.Core.Contracts;
using Mystique.Core.Device;

namespace MystiqueService;

/// <summary>Injectable so tests run without real time passing.</summary>
public delegate Task DelayAsync(TimeSpan duration, CancellationToken cancellationToken);

public sealed record SupervisorOptions(byte Rotation = 0, int SummaryEveryUpdates = 300);

/// <summary>
/// The state machine that keeps the panel alive. Its contract is simple and
/// absolute: it does not exit. Every failure becomes a state, a log line, and a
/// retry. Nothing here calls Environment.Exit.
/// </summary>
public sealed class Supervisor
{
    private readonly Func<IMystiqueDevice> deviceFactory;
    private readonly Func<CancellationToken, Task<ISensorSource>> sensorFactory;
    private readonly DeepCoolWatcher deepCool;
    private readonly ILog log;
    private readonly TimeProvider time;
    private readonly DelayAsync delay;
    private readonly SupervisorOptions options;

    // Separate counters per subsystem: a chatty sensor bridge must not push USB
    // retries to the cap.
    private readonly BackoffSchedule deviceFast = BackoffSchedule.Fast();
    private readonly BackoffSchedule deviceSlow = BackoffSchedule.Slow();
    private readonly BackoffSchedule sensorFast = BackoffSchedule.Fast();
    private readonly FailureEscalator deviceFailures;
    private readonly FailureEscalator sensorFailures;

    private IMystiqueDevice? device;
    private ISensorSource? sensors;
    private byte rotation;
    private volatile bool reinitRequested;
    private long updatesSent;
    private DateTimeOffset stateSince;

    public Supervisor(
        Func<IMystiqueDevice> deviceFactory,
        Func<CancellationToken, Task<ISensorSource>> sensorFactory,
        DeepCoolWatcher deepCool,
        ILog log,
        TimeProvider time,
        DelayAsync delay,
        SupervisorOptions options)
    {
        this.deviceFactory = deviceFactory;
        this.sensorFactory = sensorFactory;
        this.deepCool = deepCool;
        this.log = log;
        this.time = time;
        this.delay = delay;
        this.options = options;

        deviceFailures = new FailureEscalator(time);
        sensorFailures = new FailureEscalator(time);
        rotation = options.Rotation;
        stateSince = time.GetUtcNow();
        Status = StatusSnapshot.Initial(stateSince, rotation);
    }

    public StatusSnapshot Status { get; private set; }

    public event Action<StatusSnapshot>? StatusChanged;

    public void SetRotation(byte value)
    {
        if (value > 3)
            return;

        rotation = value;
        reinitRequested = true;
        log.Info($"rotation set to {value * 90} degrees; reinitialising");
    }

    public void RequestRetryNow()
    {
        deviceFast.Reset();
        deviceSlow.Reset();
        sensorFast.Reset();
        reinitRequested = true;
        log.Info("manual retry requested");
    }

    public async Task RunAsync(CancellationToken cancellationToken)
    {
        Transition(ServiceState.Starting, "service starting");

        try
        {
            while (!cancellationToken.IsCancellationRequested)
                await StepAsync(cancellationToken);
        }
        catch (OperationCanceledException) when (cancellationToken.IsCancellationRequested)
        {
            // Normal stop.
        }
        finally
        {
            ReleaseDevice("service stopping");
            ReleaseSensors();
            Transition(ServiceState.Stopped, "service stopped");
        }
    }

    private async Task StepAsync(CancellationToken cancellationToken)
    {
        try
        {
            if (await HandleDeepCoolAsync(cancellationToken))
                return;

            if (!await EnsureDeviceAsync(cancellationToken))
                return;

            if (!await EnsureSensorsAsync(cancellationToken))
                return;

            await PumpOneFrameAsync(cancellationToken);
        }
        catch (OperationCanceledException) when (cancellationToken.IsCancellationRequested)
        {
            throw;
        }
        catch (Exception ex)
        {
            // Deliberate catch-all. An unclassified bug must degrade the panel, not
            // terminate a service that is supposed to run from boot to shutdown.
            if (deviceFailures.OnFailure() is { } level)
                log.Write(level, $"unhandled step failure: {ex.GetType().Name}: {ex.Message}");

            ReleaseDevice("unhandled failure");
            Transition(ServiceState.Reconnecting, "recovering from unhandled failure");
            await delay(deviceSlow.NextDelay(), cancellationToken);
        }
    }

    private async Task<bool> HandleDeepCoolAsync(CancellationToken cancellationToken)
    {
        DeepCoolTransition transition = deepCool.Poll();

        if (transition == DeepCoolTransition.Appeared)
        {
            // Release pre-emptively. If we are already gone there is no handle for
            // DeepCool to tear down, and no reset war.
            ReleaseDevice("DeepCool.exe started");
            Transition(ServiceState.Yielded, "yielded to DeepCool.exe");
        }
        else if (transition == DeepCoolTransition.Exited)
        {
            log.Info("DeepCool.exe exited; reacquiring device");
            deviceFast.Reset();
            await delay(DeepCoolWatcher.SettleDelay, cancellationToken);
            return false;
        }

        if (!deepCool.IsPresent)
            return false;

        if (deepCool.ShouldLogHeartbeat())
            log.Info("still yielded to DeepCool.exe");

        await delay(DeepCoolWatcher.PollInterval, cancellationToken);
        return true;
    }

    private async Task<bool> EnsureDeviceAsync(CancellationToken cancellationToken)
    {
        if (device is not null && !reinitRequested)
            return true;

        if (device is not null && reinitRequested)
        {
            reinitRequested = false;
            try
            {
                device.EnterMachineInfoMode(rotation);
                return true;
            }
            catch (Exception ex)
            {
                log.Warn($"reinitialisation failed: {ex.Message}");
                ReleaseDevice("reinitialisation failed");
            }
        }

        try
        {
            IMystiqueDevice opened = deviceFactory();
            try
            {
                // A reopened device is not in Machine Info mode. Writes would be
                // ACKed while the panel showed nothing, so init is part of acquiring.
                opened.EnterMachineInfoMode(rotation);
            }
            catch
            {
                opened.Dispose();
                throw;
            }

            device = opened;
            reinitRequested = false;
            deviceFailures.OnSuccess();
            deviceFast.Reset();
            deviceSlow.Reset();
            log.Info($"device acquired: {opened.RedactedPath}, rotation {rotation * 90} degrees");
            return true;
        }
        catch (MystiqueUsbException ex)
        {
            await HandleDeviceFaultAsync(ex.Fault, ex.Message, cancellationToken);
            return false;
        }
    }

    private async Task HandleDeviceFaultAsync(
        UsbFault fault, string message, CancellationToken cancellationToken)
    {
        switch (fault.Kind)
        {
            case FaultKind.Yield:
                // Not an error and not a failure counter. Either DeepCool has it, or
                // something else does; either way we wait quietly.
                Transition(ServiceState.Yielded, "device owned by another process");
                await delay(WaitDelay(), cancellationToken);
                return;

            case FaultKind.NotPresent:
            case FaultKind.SurpriseRemoval:
                Transition(ServiceState.Waiting, fault.Description);
                if (deviceFailures.OnFailure() is { } waitLevel)
                    log.Write(waitLevel, $"waiting for device: {fault.Description}");
                await delay(WaitDelay(), cancellationToken);
                return;

            default:
                Transition(ServiceState.Reconnecting, fault.Description);
                if (deviceFailures.OnFailure() is { } level)
                    log.Write(level, $"device fault: {message}");
                device?.Recover(fault.Recovery);
                await delay(deviceSlow.NextDelay(), cancellationToken);
                return;
        }
    }

    /// <summary>
    /// Fast while arrival is still plausible, then slow. Without this the fast
    /// schedule's exhaustion is never observed, and a device left unplugged (or a
    /// DeepCool window left open all afternoon) would be probed once a second
    /// forever.
    /// </summary>
    private TimeSpan WaitDelay() =>
        deviceFast.Exhausted ? deviceSlow.NextDelay() : deviceFast.NextDelay();

    private async Task<bool> EnsureSensorsAsync(CancellationToken cancellationToken)
    {
        if (sensors is not null)
            return true;

        try
        {
            sensors = await sensorFactory(cancellationToken);
            sensorFailures.OnSuccess();
            sensorFast.Reset();
            log.Info("sensor bridge connected");
            return true;
        }
        catch (OperationCanceledException) when (cancellationToken.IsCancellationRequested)
        {
            throw;
        }
        catch (Exception ex)
        {
            // The device stays held: the panel keeps its last frame rather than
            // going blank, and takeover is instant when the bridge returns.
            Transition(ServiceState.Degraded, "sensor bridge unavailable");
            if (sensorFailures.OnFailure() is { } level)
                log.Write(level, $"sensor bridge unavailable: {ex.Message}");
            await delay(sensorFast.NextDelay(), cancellationToken);
            return false;
        }
    }

    private async Task PumpOneFrameAsync(CancellationToken cancellationToken)
    {
        SensorSample sample;
        try
        {
            sample = await sensors!.ReadAsync(cancellationToken);
        }
        catch (OperationCanceledException) when (cancellationToken.IsCancellationRequested)
        {
            throw;
        }
        catch (Exception ex)
        {
            ReleaseSensors();
            Transition(ServiceState.Degraded, "sensor stream ended");
            if (sensorFailures.OnFailure() is { } level)
                log.Write(level, $"sensor read failed: {ex.Message}");
            await delay(sensorFast.NextDelay(), cancellationToken);
            return;
        }

        var frame = TelemetryFrame.FromRaw(
            sample.CpuTemperature, sample.CpuUsage, sample.MemoryLoad,
            sample.GpuTemperature, sample.CpuClock);

        try
        {
            device!.SendTelemetry(frame);
        }
        catch (MystiqueUsbException ex)
        {
            if (ex.Fault.Kind is FaultKind.SurpriseRemoval or FaultKind.NotPresent
                or FaultKind.Yield or FaultKind.Programming)
                ReleaseDevice(ex.Fault.Description);

            await HandleDeviceFaultAsync(ex.Fault, ex.Message, cancellationToken);
            return;
        }

        updatesSent++;
        deviceSlow.RecordSuccess();
        deviceFast.RecordSuccess();
        deviceFailures.OnSuccess();

        Publish(ServiceState.Streaming, "streaming", frame);

        if (options.SummaryEveryUpdates > 0 && updatesSent % options.SummaryEveryUpdates == 0)
            log.Info(
                $"{updatesSent} updates sent; CPU {frame.CpuTemperatureC}C {frame.CpuUsagePercent}%, " +
                $"GPU {frame.GpuTemperatureC}C, RAM {frame.MemoryLoadPercent}%");
    }

    private void ReleaseDevice(string reason)
    {
        if (device is null)
            return;

        try
        {
            device.AbortPendingTransfers();
        }
        catch
        {
            // Best effort: we are giving the handle up regardless.
        }

        device.Dispose();
        device = null;
        log.Info($"device released: {reason}");
    }

    private void ReleaseSensors()
    {
        sensors?.Dispose();
        sensors = null;
    }

    private void Transition(ServiceState state, string detail)
    {
        if (Status.State == state && Status.Detail == detail)
            return;

        DateTimeOffset now = time.GetUtcNow();
        TimeSpan inPrevious = now - stateSince;
        if (Status.State != state)
            log.Info($"{Status.State} -> {state}: {detail} (after {inPrevious.TotalSeconds:F1}s)");

        stateSince = now;
        Publish(state, detail, Status.Last);
    }

    private void Publish(ServiceState state, string detail, TelemetryFrame? frame)
    {
        Status = new StatusSnapshot(state, detail, updatesSent, stateSince, rotation, frame);
        StatusChanged?.Invoke(Status);
    }
}
