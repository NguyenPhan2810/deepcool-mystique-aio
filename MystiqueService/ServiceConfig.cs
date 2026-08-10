using System.Text.Json;

namespace MystiqueService;

/// <summary>
/// Persisted service settings. Deliberately tiny: anything the tray can change
/// must survive a reconnect and a reboot, and nothing else belongs here.
/// </summary>
public sealed record ServiceConfig(byte Rotation = 0)
{
    public static string DefaultPath => Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.CommonApplicationData),
        "Mystique",
        "config.json");

    /// <summary>Never throws. A bad config must not stop the panel from lighting up.</summary>
    public static ServiceConfig Load(string path)
    {
        try
        {
            if (!File.Exists(path))
                return new ServiceConfig();

            ServiceConfig? loaded = JsonSerializer.Deserialize<ServiceConfig>(File.ReadAllText(path));
            if (loaded is null || loaded.Rotation > 3)
                return new ServiceConfig();

            return loaded;
        }
        catch (Exception ex) when (ex is IOException or JsonException or UnauthorizedAccessException)
        {
            return new ServiceConfig();
        }
    }

    public void Save(string path)
    {
        string? directory = Path.GetDirectoryName(path);
        if (!string.IsNullOrEmpty(directory))
            Directory.CreateDirectory(directory);

        File.WriteAllText(path, JsonSerializer.Serialize(
            this, new JsonSerializerOptions { WriteIndented = true }));
    }
}
