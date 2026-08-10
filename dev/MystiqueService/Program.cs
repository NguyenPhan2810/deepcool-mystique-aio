using System.ServiceProcess;
using Mystique.Core.Device;

namespace MystiqueService;

internal static class Program
{
    private const string Usage =
        "Usage: MystiqueService [--service|--console|install|uninstall|status]";

    private static int Main(string[] args)
    {
        // Explicit flag, not Environment.UserInteractive: that property is
        // unreliable on modern Windows and picking the wrong mode fails opaquely.
        string mode = args.Length == 0 ? "--console" : args[0].ToLowerInvariant();

        return mode switch
        {
            "--service" => RunAsService(),
            "--console" => RunInConsole(),
            "install" => ServiceInstaller.Install(),
            "uninstall" => ServiceInstaller.Uninstall(),
            "status" => ServiceInstaller.PrintStatus(),
            _ => Fail(),
        };
    }

    private static int Fail()
    {
        Console.Error.WriteLine(Usage);
        return 2;
    }

    private static int RunAsService()
    {
        // LocalSystem's working directory is system32; make relative paths unambiguous.
        Directory.SetCurrentDirectory(AppContext.BaseDirectory);

        var log = new RollingFileLog(RollingFileLog.DefaultDirectory, TimeProvider.System);
        Supervisor supervisor = BuildSupervisor(log);
        using var service = new MystiqueWindowsService(log, supervisor);
        ServiceBase.Run(service);
        return 0;
    }

    private static int RunInConsole()
    {
        var log = new CompositeLog(
            new RollingFileLog(RollingFileLog.DefaultDirectory, TimeProvider.System),
            new ConsoleLog());
        Supervisor supervisor = BuildSupervisor(log);

        using var stopping = new CancellationTokenSource();
        Console.CancelKeyPress += (_, e) =>
        {
            e.Cancel = true;
            stopping.Cancel();
        };

        Console.WriteLine("Running in console mode. Ctrl+C to stop.");
        supervisor.RunAsync(stopping.Token).GetAwaiter().GetResult();
        return 0;
    }

    private static Supervisor BuildSupervisor(ILog log)
    {
        ServiceConfig config = ServiceConfig.Load(ServiceConfig.DefaultPath);

        static async Task<ISensorSource> ConnectSensorsAsync(CancellationToken cancellationToken) =>
            await SensorBridgeClient.ConnectAsync(cancellationToken);

        return new Supervisor(
            deviceFactory: MystiqueDevice.Open,
            sensorFactory: ConnectSensorsAsync,
            deepCool: new DeepCoolWatcher(DeepCoolWatcher.ProcessIsRunning, TimeProvider.System),
            log: log,
            time: TimeProvider.System,
            delay: Task.Delay,
            options: new SupervisorOptions(config.Rotation));
    }
}
