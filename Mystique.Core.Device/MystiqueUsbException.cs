namespace Mystique.Core.Device;

/// <summary>
/// A USB operation failed, carrying the classification the supervisor needs.
/// Every WinUSB failure path in this assembly raises this rather than a bare
/// Win32Exception, so no caller has to re-derive the handling decision.
/// </summary>
public sealed class MystiqueUsbException : Exception
{
    public MystiqueUsbException(UsbFault fault, string operation)
        : base($"{operation} failed: {fault.Description} (Win32 {fault.Code})")
    {
        Fault = fault;
        Operation = operation;
    }

    public UsbFault Fault { get; }

    public string Operation { get; }
}
