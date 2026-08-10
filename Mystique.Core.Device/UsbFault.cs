namespace Mystique.Core.Device;

public enum FaultKind
{
    /// <summary>Device is not enumerated. Normal at boot; not an error.</summary>
    NotPresent,

    /// <summary>Someone else owns the handle. We step aside by design.</summary>
    Yield,

    /// <summary>Recoverable transfer-level failure.</summary>
    Transient,

    /// <summary>Could be either; treat conservatively.</summary>
    Ambiguous,

    /// <summary>The device physically went away mid-transfer.</summary>
    SurpriseRemoval,

    /// <summary>We cancelled the transfer ourselves.</summary>
    SelfAborted,

    /// <summary>A bug on our side, not the device's.</summary>
    Programming,
}

public enum RecoveryAction
{
    None,
    AbortAndFlush,
    AbortFlushReset,
    Reopen,
    WaitForArrival,
}

/// <summary>
/// Maps a Win32 error code to what the supervisor should do about it.
/// Unknown codes are handled conservatively rather than fatally: nothing in this
/// type ever terminates the service.
/// </summary>
public readonly record struct UsbFault(
    int Code,
    FaultKind Kind,
    RecoveryAction Recovery,
    bool UseFastBackoff,
    string Description)
{
    public const int FileNotFound = 2;
    public const int AccessDenied = 5;
    public const int InvalidHandle = 6;
    public const int GeneralFailure = 31;
    public const int SharingViolation = 32;
    public const int SemTimeout = 121;
    public const int NoSuchDevice = 433;
    public const int OperationAborted = 995;
    public const int DeviceNotConnected = 1167;

    public static UsbFault Classify(int win32Code, bool weInitiatedAbort = false) => win32Code switch
    {
        FileNotFound => new(win32Code, FaultKind.NotPresent, RecoveryAction.WaitForArrival, true,
            "device not enumerated yet"),

        AccessDenied or SharingViolation => new(win32Code, FaultKind.Yield, RecoveryAction.None, true,
            "another process owns the device"),

        SemTimeout => new(win32Code, FaultKind.Transient, RecoveryAction.AbortAndFlush, false,
            "pipe transfer timed out"),

        GeneralFailure => new(win32Code, FaultKind.Ambiguous, RecoveryAction.AbortFlushReset, false,
            "general device failure"),

        NoSuchDevice or DeviceNotConnected => new(
            win32Code, FaultKind.SurpriseRemoval, RecoveryAction.WaitForArrival, true,
            "device removed"),

        OperationAborted when weInitiatedAbort => new(
            win32Code, FaultKind.SelfAborted, RecoveryAction.None, true,
            "transfer cancelled by us"),

        OperationAborted => new(win32Code, FaultKind.SurpriseRemoval, RecoveryAction.WaitForArrival, true,
            "transfer aborted by the stack"),

        InvalidHandle => new(win32Code, FaultKind.Programming, RecoveryAction.Reopen, false,
            "invalid handle - use after free"),

        _ => new(win32Code, FaultKind.Ambiguous, RecoveryAction.AbortFlushReset, false,
            $"unclassified Win32 error {win32Code}"),
    };
}
