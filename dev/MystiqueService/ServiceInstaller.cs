using System.Diagnostics;
using System.Security.Principal;
using System.ServiceProcess;

namespace MystiqueService;

/// <summary>
/// Registers the service via sc.exe. Chosen over P/Invoking CreateService because
/// every step below is inspectable and reversible with one command by hand.
/// </summary>
internal static class ServiceInstaller
{
    internal static string InstallDirectory => Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles), "Mystique");

    internal static int Install()
    {
        if (!IsElevated())
        {
            Console.Error.WriteLine("Run from an elevated prompt: registering a service requires admin.");
            return 3;
        }

        string exe = Path.Combine(InstallDirectory, "MystiqueService.exe");
        if (!File.Exists(exe))
        {
            Console.Error.WriteLine($"Not found: {exe}");
            Console.Error.WriteLine(
                $"Publish first: dotnet publish MystiqueService -c Release -o \"{InstallDirectory}\"");
            return 4;
        }

        // SERVICE_AUTO_START, not delayed-auto: delayed would push the first frame
        // out by roughly two minutes.
        //
        // No DependOnService. Measured: the Deep Cool Helper Service reports RUNNING
        // at boot+20.0 s but its pipe does not exist until boot+24.2 s. SCM waits for
        // RUNNING, not for ready, so the dependency buys nothing the retry loop must
        // provide anyway - while adding a hard failure if DeepCool renames it.
        // sc.exe wants each "name=" and its value as SEPARATE argv tokens. Passing
        // "binPath= <value>" as one token makes sc reject the whole command.
        // The inner exe path stays quoted because Program Files contains a space.
        Run("create", MystiqueWindowsService.WindowsServiceName,
            "binPath=", $"\"{exe}\" --service",
            "start=", "auto",
            "DisplayName=", "Mystique LCD Telemetry");

        Run("description", MystiqueWindowsService.WindowsServiceName,
            "Drives the DeepCool Mystique LCD with system telemetry from boot.");

        // unrestricted gives us NT SERVICE\MystiqueService for ACLs without removing
        // any group SIDs. Never 'restricted' - that adds a write-restricted token and
        // is a realistic way to break WinUSB access.
        Run("sidtype", MystiqueWindowsService.WindowsServiceName, "unrestricted");

        // Crash backstop, distinct from the in-process reconnect loop.
        Run("failure", MystiqueWindowsService.WindowsServiceName,
            "reset=", "86400",
            "actions=", "restart/5000/restart/10000/restart/30000");

        Run("start", MystiqueWindowsService.WindowsServiceName);

        Console.WriteLine("Installed and started.");
        return PrintStatus();
    }

    internal static int Uninstall()
    {
        if (!IsElevated())
        {
            Console.Error.WriteLine("Run from an elevated prompt.");
            return 3;
        }

        Run("stop", allowFailure: true, MystiqueWindowsService.WindowsServiceName);
        Run("delete", MystiqueWindowsService.WindowsServiceName);
        Console.WriteLine("Uninstalled. Log files under %ProgramData%\\Mystique were left in place.");
        return 0;
    }

    internal static int PrintStatus()
    {
        try
        {
            using var controller = new ServiceController(MystiqueWindowsService.WindowsServiceName);
            Console.WriteLine($"{controller.ServiceName}: {controller.Status}");
            return 0;
        }
        catch (InvalidOperationException)
        {
            Console.WriteLine($"{MystiqueWindowsService.WindowsServiceName}: not installed");
            return 1;
        }
    }

    private static bool IsElevated()
    {
        using var identity = WindowsIdentity.GetCurrent();
        return new WindowsPrincipal(identity).IsInRole(WindowsBuiltInRole.Administrator);
    }

    private static void Run(string verb, params string[] rest) => Run(verb, false, rest);

    private static void Run(string verb, bool allowFailure, params string[] rest)
    {
        var startInfo = new ProcessStartInfo("sc.exe")
        {
            UseShellExecute = false,
            RedirectStandardOutput = true,
            RedirectStandardError = true,
        };
        startInfo.ArgumentList.Add(verb);
        foreach (string argument in rest)
            startInfo.ArgumentList.Add(argument);

        using Process process = Process.Start(startInfo)
            ?? throw new InvalidOperationException("Could not start sc.exe");

        string output = process.StandardOutput.ReadToEnd();
        string error = process.StandardError.ReadToEnd();
        process.WaitForExit();

        if (process.ExitCode == 0)
            return;

        // sc.exe reports the real reason on stdout, so surface it rather than
        // leaving the caller with a bare exit code to guess from.
        string detail = string.Join(' ', $"{output} {error}".Split(
            (char[]?)null, StringSplitOptions.RemoveEmptyEntries));

        if (!allowFailure)
            throw new InvalidOperationException(
                $"sc.exe {verb} failed with exit code {process.ExitCode}: {detail}");

        Console.WriteLine($"(sc.exe {verb} returned {process.ExitCode}: {detail})");
    }
}
