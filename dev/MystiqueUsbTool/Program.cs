using Mystique.Core.Contracts;
using Mystique.Core.Device;

namespace MystiqueUsbTool;

/// <summary>
/// Diagnostic CLI over Mystique.Core. Deliberately thin: all protocol and device
/// logic lives in Core so the service and this tool cannot drift apart.
/// </summary>
internal static class Program
{
    private const string Usage =
        "Usage: MystiqueUsbTool [pipes|status|init [rotation]|info|stream [seconds]|live [updates]] " +
        "[--show-device-id]";

    private static async Task<int> Main(string[] args)
    {
        bool showDeviceId = args.Contains("--show-device-id", StringComparer.OrdinalIgnoreCase);
        string[] positional = args
            .Where(a => !a.StartsWith("--", StringComparison.Ordinal))
            .ToArray();

        string operation = positional.Length == 0 ? "pipes" : positional[0].ToLowerInvariant();
        if (operation is not ("pipes" or "status" or "init" or "info" or "stream" or "live"))
        {
            Console.Error.WriteLine(Usage);
            return 2;
        }

        try
        {
            using MystiqueDevice device = MystiqueDevice.Open();
            Console.WriteLine($"Device: {device.RedactedPath}");
            PrintPipes(device);

            switch (operation)
            {
                case "pipes":
                    break;

                case "status":
                    PrintStatus(device.ReadStatus());
                    break;

                case "info":
                    PrintInfo(device, showDeviceId);
                    break;

                case "init":
                    RunInit(device, ReadRotation(positional), showDeviceId);
                    break;

                case "stream":
                    StreamSynthetic(device, ReadCount(positional, 30, 1, 600));
                    break;

                case "live":
                    RunInit(device, ReadRotation(positional), showDeviceId);
                    await StreamLiveAsync(device, ReadCount(positional, 0, 0, 86_400));
                    break;
            }

            return 0;
        }
        catch (MystiqueUsbException ex)
        {
            Console.Error.WriteLine($"ERROR ({ex.Fault.Kind}, Win32 {ex.Fault.Code}): {ex.Message}");
            if (ex.Fault.Kind == FaultKind.Yield)
                Console.Error.WriteLine("Close DeepCool.exe and stop MystiqueService, then retry.");
            return 1;
        }
        catch (Exception ex)
        {
            Console.Error.WriteLine($"ERROR: {ex.Message}");
            return 1;
        }
    }

    private static byte ReadRotation(string[] positional) =>
        positional.Length >= 2 && byte.TryParse(positional[1], out byte parsed) && parsed <= 3
            ? parsed
            : (byte)0;

    private static int ReadCount(string[] positional, int fallback, int minimum, int maximum) =>
        positional.Length >= 2 && int.TryParse(positional[1], out int parsed)
            ? Math.Clamp(parsed, minimum, maximum)
            : fallback;

    private static void PrintPipes(IMystiqueDevice device)
    {
        var pipes = device.DescribePipes();
        Console.WriteLine($"Interface 0: {pipes.Count} endpoints");
        foreach (PipeInfo pipe in pipes)
            Console.WriteLine($"  0x{pipe.PipeId:X2} {pipe.Type,-9} max-packet={pipe.MaximumPacketSize}");
    }

    private static void PrintStatus(DeviceStatus status) =>
        Console.WriteLine(
            $"Status: mode={status.Mode} (0x{status.RawMode:X2}), rotation={status.RotationDegrees} degrees");

    private static void PrintInfo(IMystiqueDevice device, bool showDeviceId)
    {
        // Redacted unless explicitly requested: bytes 4-11 are a per-device identifier.
        byte[] response = device.SendInfoRequest();
        Console.WriteLine($"Info hex:  {MystiquePackets.FormatResponse(response, showDeviceId)}");
        Console.WriteLine($"Info text: {MystiquePackets.FormatResponseAscii(response, showDeviceId)}");
        if (!showDeviceId)
            Console.WriteLine("(device identifier redacted; pass --show-device-id to reveal)");
    }

    private static void RunInit(IMystiqueDevice device, byte rotation, bool showDeviceId)
    {
        Console.WriteLine(
            $"Sending reversible Machine Info initialization (rotation {rotation * 90} degrees)...");
        foreach (InitStep step in device.EnterMachineInfoMode(rotation))
        {
            Console.WriteLine(
                $"  command 0x{step.Command:X2}: " +
                $"{MystiquePackets.FormatResponse(step.Response, showDeviceId)}");
        }

        PrintStatus(device.ReadStatus());
    }

    private static void StreamSynthetic(IMystiqueDevice device, int seconds)
    {
        Console.WriteLine($"Sending {seconds} one-per-second synthetic telemetry updates...");
        for (int second = 0; second < seconds; second++)
        {
            var frame = new TelemetryFrame(
                CpuTemperatureC: (byte)(35 + second % 25),
                CpuUsagePercent: (byte)(10 + second % 80),
                MemoryLoadPercent: (byte)(40 + second % 20),
                GpuTemperatureC: (byte)(45 + second % 20),
                CpuClockMhz: (ushort)(4200 + second % 8 * 100));

            device.SendTelemetry(frame);
            Console.WriteLine(
                $"  {second + 1,3}/{seconds}: CPU {frame.CpuTemperatureC}C {frame.CpuUsagePercent}%  " +
                $"RAM {frame.MemoryLoadPercent}%  GPU {frame.GpuTemperatureC}C  {frame.CpuClockMhz}MHz");
            Thread.Sleep(1000);
        }
    }

    private static async Task StreamLiveAsync(IMystiqueDevice device, int updateLimit)
    {
        using var stop = new CancellationTokenSource();
        ConsoleCancelEventHandler cancelHandler = (_, e) =>
        {
            e.Cancel = true;
            stop.Cancel();
        };
        Console.CancelKeyPress += cancelHandler;

        try
        {
            using SensorBridgeClient sensors = await SensorBridgeClient.ConnectAsync(stop.Token);
            Console.WriteLine(updateLimit == 0
                ? "Streaming live telemetry until Ctrl+C..."
                : $"Streaming {updateLimit} live telemetry updates...");

            int sent = 0;
            while (!stop.IsCancellationRequested && (updateLimit == 0 || sent < updateLimit))
            {
                SensorSample sample = await sensors.ReadAsync(stop.Token);
                var frame = TelemetryFrame.FromRaw(
                    sample.CpuTemperature, sample.CpuUsage, sample.MemoryLoad,
                    sample.GpuTemperature, sample.CpuClock);

                device.SendTelemetry(frame);
                sent++;
                Console.WriteLine(
                    $"  {sent,5}: CPU {frame.CpuTemperatureC}C {frame.CpuUsagePercent}% " +
                    $"{frame.CpuClockMhz}MHz  RAM {frame.MemoryLoadPercent}%  " +
                    $"GPU {frame.GpuTemperatureC}C");
            }
        }
        catch (OperationCanceledException) when (stop.IsCancellationRequested)
        {
            Console.WriteLine("Stopped.");
        }
        finally
        {
            Console.CancelKeyPress -= cancelHandler;
        }
    }
}
