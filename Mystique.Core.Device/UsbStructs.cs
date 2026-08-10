using System.Runtime.InteropServices;

namespace Mystique.Core.Device;

public enum UsbdPipeType
{
    Control,
    Isochronous,
    Bulk,
    Interrupt,
}

[StructLayout(LayoutKind.Sequential)]
public struct UsbInterfaceDescriptor
{
    public byte Length;
    public byte DescriptorType;
    public byte InterfaceNumber;
    public byte AlternateSetting;
    public byte NumEndpoints;
    public byte InterfaceClass;
    public byte InterfaceSubClass;
    public byte InterfaceProtocol;
    public byte Interface;
}

[StructLayout(LayoutKind.Sequential)]
public struct WinUsbPipeInformation
{
    public UsbdPipeType PipeType;
    public byte PipeId;
    public ushort MaximumPacketSize;
    public byte Interval;
}
