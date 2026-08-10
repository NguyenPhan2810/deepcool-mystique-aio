namespace Mystique.Core.Device;

/// <summary>
/// Finds the Mystique's WinUSB interface path, and answers "is it plugged in?"
/// with the same call.
///
/// <para>This deliberately does not read
/// <c>HKLM\SYSTEM\CurrentControlSet\Enum\...\Device Parameters\SymbolicName</c>.
/// That key persists across unplugs, so it returns paths for absent hardware;
/// on the development machine 22 of 35 such entries were stale.
/// <c>CM_GET_DEVICE_INTERFACE_LIST_PRESENT</c> returns present devices only.</para>
/// </summary>
public static class DeviceLocator
{
    /// <summary>
    /// GUID_DEVINTERFACE_USB_DEVICE. Confirmed to be the GUID in this device's
    /// SymbolicName; its DeviceInterfaceGUIDs value is empty, so that value must
    /// not be used as the lookup key.
    /// </summary>
    public static readonly Guid UsbDeviceInterfaceClass =
        new("a5dcbf10-6530-11d2-901f-00c04fb951ed");

    public const string HardwareIdFragment = "vid_3633&pid_0009";

    private const int MaxSizeRetries = 4;

    /// <summary>Returns the interface path, or null if the device is not present.</summary>
    public static string? FindPresentDevicePath()
    {
        foreach (string path in EnumeratePresentInterfaces())
        {
            if (IsMystiqueInterface(path))
                return path;
        }

        return null;
    }

    public static bool IsMystiqueInterface(string interfacePath) =>
        interfacePath.Contains(HardwareIdFragment, StringComparison.OrdinalIgnoreCase);

    /// <summary>Splits a Windows REG_MULTI_SZ-style double-null-terminated buffer.</summary>
    public static IReadOnlyList<string> ParseInterfaceList(ReadOnlySpan<char> multiSz)
    {
        var entries = new List<string>();
        int start = 0;

        for (int i = 0; i < multiSz.Length; i++)
        {
            if (multiSz[i] != '\0')
                continue;

            if (i == start)
                break; // empty entry marks the end of the list

            entries.Add(multiSz[start..i].ToString());
            start = i + 1;
        }

        return entries;
    }

    /// <summary>Masks the per-device serial in an interface path for logging.</summary>
    public static string RedactSerial(string interfacePath)
    {
        string[] parts = interfacePath.Split('#');
        return parts.Length >= 4
            ? string.Join('#', parts[0], parts[1], "<serial>", parts[3])
            : interfacePath;
    }

    private static IReadOnlyList<string> EnumeratePresentInterfaces()
    {
        Guid guid = UsbDeviceInterfaceClass;

        // The device set can change between sizing and fetching, so CM can return
        // CR_BUFFER_SMALL on the second call. Retry a bounded number of times.
        for (int attempt = 0; attempt < MaxSizeRetries; attempt++)
        {
            int sizeResult = Native.CM_Get_Device_Interface_List_Size(
                out uint length, ref guid, IntPtr.Zero, Native.GetInterfaceListPresent);
            if (sizeResult != Native.CrSuccess)
                throw new InvalidOperationException(
                    $"CM_Get_Device_Interface_List_Size failed with CONFIGRET 0x{sizeResult:X2}.");

            if (length <= 1)
                return [];

            char[] buffer = new char[length];
            int listResult = Native.CM_Get_Device_Interface_List(
                ref guid, IntPtr.Zero, buffer, length, Native.GetInterfaceListPresent);

            if (listResult == Native.CrSuccess)
                return ParseInterfaceList(buffer);

            if (listResult != Native.CrBufferSmall)
                throw new InvalidOperationException(
                    $"CM_Get_Device_Interface_List failed with CONFIGRET 0x{listResult:X2}.");
        }

        throw new InvalidOperationException(
            $"Device interface list kept growing across {MaxSizeRetries} attempts.");
    }
}
