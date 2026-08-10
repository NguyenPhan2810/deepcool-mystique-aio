using System.Security.AccessControl;
using System.Security.Principal;
using Mystique.Core.Device;

namespace MystiqueService;

/// <summary>
/// Timestamped rolling log plus an in-memory ring a reader can consume without
/// touching the file. Rotation is a backstop: the real volume control is not
/// logging one line per telemetry update.
/// </summary>
public sealed class RollingFileLog : ILog, IDisposable
{
    private const int RingCapacity = 200;
    private const string BaseName = "mystique";

    private readonly Lock gate = new();
    private readonly Queue<string> ring = new();
    private readonly string directory;
    private readonly string currentPath;
    private readonly TimeProvider time;
    private readonly long maxBytes;
    private readonly int generations;
    private StreamWriter writer;
    private long written;

    public RollingFileLog(
        string directory,
        TimeProvider time,
        long maxBytes = 8 * 1024 * 1024,
        int generations = 3)
    {
        this.directory = directory;
        this.time = time;
        this.maxBytes = maxBytes;
        this.generations = Math.Max(1, generations);

        EnsureSecureDirectory(directory);
        currentPath = Path.Combine(directory, $"{BaseName}.log");
        writer = Open(currentPath, out written);
    }

    public static string DefaultDirectory => Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.CommonApplicationData),
        "Mystique",
        "logs");

    /// <summary>
    /// Creates the directory with inheritance disabled and an explicit DACL.
    /// Inherited C:\ProgramData permissions grant BUILTIN\Users create-file and
    /// CREATOR OWNER GENERIC_ALL, which would let a standard user pre-create files
    /// here and own them.
    /// </summary>
    public static void EnsureSecureDirectory(string path)
    {
        Directory.CreateDirectory(path);

        var info = new DirectoryInfo(path);
        DirectorySecurity security = info.GetAccessControl();
        security.SetAccessRuleProtection(isProtected: true, preserveInheritance: false);

        foreach (FileSystemAccessRule existing in
                 security.GetAccessRules(true, false, typeof(SecurityIdentifier)))
            security.RemoveAccessRule(existing);

        Grant(security, WellKnownSidType.LocalSystemSid, FileSystemRights.FullControl);
        Grant(security, WellKnownSidType.BuiltinAdministratorsSid, FileSystemRights.FullControl);

        // Users get read-only: they may tail the log, but must not be able to
        // pre-create or tamper with files the service will later write.
        Grant(security, WellKnownSidType.BuiltinUsersSid, FileSystemRights.ReadAndExecute);

        // The identity actually running gets write access. As a service this is
        // LocalSystem and therefore redundant; in --console mode it is the invoking
        // user, who would otherwise be left with the read-only Users grant above and
        // could not open its own log.
        using (var identity = WindowsIdentity.GetCurrent())
        {
            if (identity.User is { } currentUser)
                security.AddAccessRule(new FileSystemAccessRule(
                    currentUser,
                    FileSystemRights.Modify,
                    InheritanceFlags.ContainerInherit | InheritanceFlags.ObjectInherit,
                    PropagationFlags.None,
                    AccessControlType.Allow));
        }

        info.SetAccessControl(security);
    }

    public void Write(LogLevel level, string message)
    {
        string line =
            $"{time.GetUtcNow():yyyy-MM-ddTHH:mm:ss.fffzzz} {level.ToString().ToUpperInvariant(),-5} {message}";

        lock (gate)
        {
            ring.Enqueue(line);
            while (ring.Count > RingCapacity)
                ring.Dequeue();

            writer.WriteLine(line);
            written += line.Length + Environment.NewLine.Length;

            if (written >= maxBytes)
                Rotate();
        }
    }

    public IReadOnlyList<string> Recent(int count)
    {
        lock (gate)
            return ring.TakeLast(Math.Max(0, count)).ToArray();
    }

    public void Dispose()
    {
        lock (gate)
            writer.Dispose();
    }

    private void Rotate()
    {
        writer.Dispose();

        string oldest = Path.Combine(directory, $"{BaseName}.{generations - 1}.log");
        if (File.Exists(oldest))
            File.Delete(oldest);

        for (int generation = generations - 2; generation >= 1; generation--)
        {
            string from = Path.Combine(directory, $"{BaseName}.{generation}.log");
            if (File.Exists(from))
                File.Move(from, Path.Combine(directory, $"{BaseName}.{generation + 1}.log"), overwrite: true);
        }

        if (generations > 1)
            File.Move(currentPath, Path.Combine(directory, $"{BaseName}.1.log"), overwrite: true);
        else
            File.Delete(currentPath);

        writer = Open(currentPath, out written);
    }

    private static StreamWriter Open(string path, out long length)
    {
        var stream = new FileStream(path, FileMode.Append, FileAccess.Write, FileShare.Read);
        length = stream.Length;
        return new StreamWriter(stream) { AutoFlush = true };
    }

    private static void Grant(DirectorySecurity security, WellKnownSidType sid, FileSystemRights rights) =>
        security.AddAccessRule(new FileSystemAccessRule(
            new SecurityIdentifier(sid, null),
            rights,
            InheritanceFlags.ContainerInherit | InheritanceFlags.ObjectInherit,
            PropagationFlags.None,
            AccessControlType.Allow));
}
