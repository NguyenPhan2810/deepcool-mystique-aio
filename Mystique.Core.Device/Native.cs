using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;

namespace Mystique.Core.Device;

internal static class Native
{
    internal const uint GenericRead = 0x80000000;
    internal const uint GenericWrite = 0x40000000;
    internal const uint FileShareRead = 0x00000001;
    internal const uint FileShareWrite = 0x00000002;
    internal const uint OpenExisting = 3;
    internal const uint FileAttributeNormal = 0x00000080;
    internal const uint FileFlagOverlapped = 0x40000000;

    // WINUSB_PIPE_POLICY identifiers.
    internal const uint PipeTransferTimeout = 0x03;
    internal const uint ResetPipeOnResume = 0x09;

    // CONFIGRET values.
    internal const int CrSuccess = 0x00;
    internal const int CrBufferSmall = 0x1A;

    // CM_GET_DEVICE_INTERFACE_LIST_PRESENT is 0; ALL_DEVICES is 1. Present-only is
    // the entire point of using this API over the registry.
    internal const uint GetInterfaceListPresent = 0x00000000;

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    internal static extern SafeFileHandle CreateFile(
        string fileName,
        uint desiredAccess,
        uint shareMode,
        IntPtr securityAttributes,
        uint creationDisposition,
        uint flagsAndAttributes,
        IntPtr templateFile);

    [DllImport("cfgmgr32.dll", CharSet = CharSet.Unicode, EntryPoint = "CM_Get_Device_Interface_List_SizeW")]
    internal static extern int CM_Get_Device_Interface_List_Size(
        out uint length, ref Guid interfaceClassGuid, IntPtr deviceId, uint flags);

    [DllImport("cfgmgr32.dll", CharSet = CharSet.Unicode, EntryPoint = "CM_Get_Device_Interface_ListW")]
    internal static extern int CM_Get_Device_Interface_List(
        ref Guid interfaceClassGuid, IntPtr deviceId, char[] buffer, uint bufferLength, uint flags);

    [DllImport("winusb.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool WinUsb_Initialize(SafeFileHandle deviceHandle, out IntPtr interfaceHandle);

    [DllImport("winusb.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool WinUsb_Free(IntPtr interfaceHandle);

    [DllImport("winusb.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool WinUsb_QueryInterfaceSettings(
        IntPtr interfaceHandle, byte alternateInterfaceNumber,
        out UsbInterfaceDescriptor usbAltInterfaceDescriptor);

    [DllImport("winusb.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool WinUsb_QueryPipe(
        IntPtr interfaceHandle, byte alternateInterfaceNumber, byte pipeIndex,
        out WinUsbPipeInformation pipeInformation);

    /// <summary>ULONG-valued policies, e.g. PIPE_TRANSFER_TIMEOUT.</summary>
    [DllImport("winusb.dll", SetLastError = true, EntryPoint = "WinUsb_SetPipePolicy")]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool WinUsb_SetPipePolicyUInt32(
        IntPtr interfaceHandle, byte pipeId, uint policyType, uint valueLength, ref uint value);

    /// <summary>BOOL-valued policies, e.g. RESET_PIPE_ON_RESUME. Value length is 1, not 4.</summary>
    [DllImport("winusb.dll", SetLastError = true, EntryPoint = "WinUsb_SetPipePolicy")]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool WinUsb_SetPipePolicyByte(
        IntPtr interfaceHandle, byte pipeId, uint policyType, uint valueLength, ref byte value);

    [DllImport("winusb.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool WinUsb_WritePipe(
        IntPtr interfaceHandle, byte pipeId, byte[] buffer, uint bufferLength,
        out uint lengthTransferred, IntPtr overlapped);

    [DllImport("winusb.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool WinUsb_ReadPipe(
        IntPtr interfaceHandle, byte pipeId, byte[] buffer, uint bufferLength,
        out uint lengthTransferred, IntPtr overlapped);

    [DllImport("winusb.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool WinUsb_AbortPipe(IntPtr interfaceHandle, byte pipeId);

    [DllImport("winusb.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool WinUsb_FlushPipe(IntPtr interfaceHandle, byte pipeId);

    [DllImport("winusb.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool WinUsb_ResetPipe(IntPtr interfaceHandle, byte pipeId);
}
