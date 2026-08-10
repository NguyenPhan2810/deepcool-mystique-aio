using Mystique.Core.Device;

namespace MystiqueService;

public interface ILog
{
    void Write(LogLevel level, string message);

    /// <summary>Newest lines, oldest first. Served from memory so a reader never touches the file.</summary>
    IReadOnlyList<string> Recent(int count);
}

public static class LogExtensions
{
    public static void Debug(this ILog log, string message) => log.Write(LogLevel.Debug, message);

    public static void Info(this ILog log, string message) => log.Write(LogLevel.Info, message);

    public static void Warn(this ILog log, string message) => log.Write(LogLevel.Warn, message);

    public static void Error(this ILog log, string message) => log.Write(LogLevel.Error, message);
}

public sealed class ConsoleLog : ILog
{
    private const int RingCapacity = 200;

    private readonly Lock gate = new();
    private readonly Queue<string> recent = new();

    public void Write(LogLevel level, string message)
    {
        string line =
            $"{DateTimeOffset.Now:yyyy-MM-ddTHH:mm:ss.fffzzz} {level.ToString().ToUpperInvariant(),-5} {message}";

        lock (gate)
        {
            recent.Enqueue(line);
            while (recent.Count > RingCapacity)
                recent.Dequeue();
        }

        if (level >= LogLevel.Warn)
            Console.Error.WriteLine(line);
        else
            Console.WriteLine(line);
    }

    public IReadOnlyList<string> Recent(int count)
    {
        lock (gate)
            return recent.TakeLast(Math.Max(0, count)).ToArray();
    }
}

public sealed class CompositeLog(params ILog[] sinks) : ILog
{
    public void Write(LogLevel level, string message)
    {
        foreach (ILog sink in sinks)
            sink.Write(level, message);
    }

    public IReadOnlyList<string> Recent(int count) =>
        sinks.Length == 0 ? [] : sinks[0].Recent(count);
}
