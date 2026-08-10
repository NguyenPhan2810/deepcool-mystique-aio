using System.ServiceProcess;
using Mystique.Core.Contracts;

namespace MystiqueService;

public sealed class MystiqueWindowsService : ServiceBase
{
    public const string WindowsServiceName = "MystiqueService";

    private readonly ILog log;
    private readonly Supervisor supervisor;
    private readonly CancellationTokenSource stopping = new();
    private Task? worker;

    public MystiqueWindowsService(ILog log, Supervisor supervisor)
    {
        this.log = log;
        this.supervisor = supervisor;

        ServiceName = WindowsServiceName;
        // Defaults to false. Without it SERVICE_CONTROL_SHUTDOWN never arrives and
        // the process is simply killed at shutdown.
        CanShutdown = true;
        CanStop = true;
        CanPauseAndContinue = false;
        AutoLog = false;
    }

    protected override void OnStart(string[] args)
    {
        log.Info($"service starting (pid {Environment.ProcessId})");
        worker = Task.Run(() => supervisor.RunAsync(stopping.Token));
    }

    protected override void OnStop() => Shutdown("SCM stop");

    protected override void OnShutdown() => Shutdown("system shutdown");

    protected override void OnCustomCommand(int command)
    {
        // The only untrusted input the service accepts is an integer in this fixed
        // set. There is no payload, no path, and deliberately no raw-packet command.
        if (MystiqueCommand.ToRotation(command) is { } rotation)
        {
            supervisor.SetRotation(rotation);
            new ServiceConfig(rotation).Save(ServiceConfig.DefaultPath);
            return;
        }

        switch (command)
        {
            case MystiqueCommand.RotateNext:
                byte next = (byte)((supervisor.Status.Rotation + 1) % 4);
                supervisor.SetRotation(next);
                new ServiceConfig(next).Save(ServiceConfig.DefaultPath);
                break;

            case MystiqueCommand.RetryNow:
            case MystiqueCommand.Reacquire:
                supervisor.RequestRetryNow();
                break;

            case MystiqueCommand.ReloadConfig:
                supervisor.SetRotation(ServiceConfig.Load(ServiceConfig.DefaultPath).Rotation);
                break;

            default:
                log.Warn($"ignoring unknown custom command {command}");
                break;
        }
    }

    private void Shutdown(string reason)
    {
        log.Info($"stopping: {reason}");
        stopping.Cancel();

        // Nothing is sent to the LCD here. WaitToKillServiceTimeout is 5000 ms and a
        // mode reset is 13 exchanges; attempting one would guarantee a force-kill
        // mid-sequence and leave the device in an indeterminate state rather than a
        // known one.
        try
        {
            worker?.Wait(TimeSpan.FromSeconds(3));
        }
        catch (AggregateException)
        {
            // Cancellation surfaces here; the handle is released either way.
        }

        log.Info("stopped");
    }

    protected override void Dispose(bool disposing)
    {
        if (disposing)
        {
            stopping.Dispose();
            (log as IDisposable)?.Dispose();
        }

        base.Dispose(disposing);
    }
}
