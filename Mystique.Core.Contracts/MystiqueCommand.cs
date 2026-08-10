namespace Mystique.Core.Contracts;

/// <summary>
/// SCM custom control codes the tray may send. Windows restricts these to
/// principals holding SERVICE_USER_DEFINED_CONTROL, so the service parses no
/// untrusted bytes - the only input is an integer in this fixed set.
///
/// Deliberately absent: any command carrying a payload, a file path, or a raw
/// packet. Such a command would be a local privilege-escalation primitive
/// against a LocalSystem service.
/// </summary>
public static class MystiqueCommand
{
    public const int RotateNext = 128;
    public const int Rotate0 = 129;
    public const int Rotate90 = 130;
    public const int Rotate180 = 131;
    public const int Rotate270 = 132;
    public const int ReleaseToDeepCool = 133;
    public const int Reacquire = 134;
    public const int RetryNow = 135;
    public const int ReloadConfig = 136;

    /// <summary>Maps a rotation command to its wire value, or null if not a rotation command.</summary>
    public static byte? ToRotation(int command) => command switch
    {
        Rotate0 => 0,
        Rotate90 => 1,
        Rotate180 => 2,
        Rotate270 => 3,
        _ => null,
    };
}
