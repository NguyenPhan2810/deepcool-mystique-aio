using System.Diagnostics;

namespace MystiqueService;

public enum DeepCoolTransition
{
    Unchanged,
    Appeared,
    Exited,
}

/// <summary>
/// Watches for DeepCool.exe so the supervisor can step aside before it takes the
/// device, rather than discovering the loss as an error.
///
/// <para>Only DeepCool.exe matters here. The two DeepCool <i>services</i> stay
/// running and supply the sensor data we consume; they are not competitors.</para>
/// </summary>
public sealed class DeepCoolWatcher(Func<bool> isRunning, TimeProvider time)
{
    public static readonly TimeSpan PollInterval = TimeSpan.FromSeconds(2);
    public static readonly TimeSpan HeartbeatInterval = TimeSpan.FromMinutes(15);

    /// <summary>How long to let the device settle after DeepCool exits before reopening.</summary>
    public static readonly TimeSpan SettleDelay = TimeSpan.FromSeconds(3);

    private bool present;
    private DateTimeOffset lastHeartbeat;

    public bool IsPresent => present;

    public DeepCoolTransition Poll()
    {
        bool now = isRunning();
        if (now == present)
            return DeepCoolTransition.Unchanged;

        present = now;
        if (present)
        {
            lastHeartbeat = time.GetUtcNow();
            return DeepCoolTransition.Appeared;
        }

        return DeepCoolTransition.Exited;
    }

    /// <summary>
    /// True at most once per heartbeat interval while yielded. Yielding is a normal
    /// state, so it gets a slow heartbeat rather than repeated warnings.
    /// </summary>
    public bool ShouldLogHeartbeat()
    {
        if (!present)
            return false;

        DateTimeOffset now = time.GetUtcNow();
        if (now - lastHeartbeat < HeartbeatInterval)
            return false;

        lastHeartbeat = now;
        return true;
    }

    public static bool ProcessIsRunning()
    {
        Process[] found = Process.GetProcessesByName("DeepCool");
        try
        {
            return found.Length > 0;
        }
        finally
        {
            foreach (Process process in found)
                process.Dispose();
        }
    }
}
