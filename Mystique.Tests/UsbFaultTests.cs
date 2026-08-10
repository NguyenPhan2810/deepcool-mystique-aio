using Mystique.Core.Device;

namespace Mystique.Tests;

public class UsbFaultTests
{
    [Fact]
    public void FileNotFoundIsNormalAtBootNotAFault()
    {
        var fault = UsbFault.Classify(2);

        Assert.Equal(FaultKind.NotPresent, fault.Kind);
        Assert.Equal(RecoveryAction.WaitForArrival, fault.Recovery);
        Assert.True(fault.UseFastBackoff);
    }

    [Theory]
    [InlineData(5)]   // ERROR_ACCESS_DENIED
    [InlineData(32)]  // ERROR_SHARING_VIOLATION
    public void AnotherOwnerIsAYieldNotAnError(int code)
    {
        var fault = UsbFault.Classify(code);

        Assert.Equal(FaultKind.Yield, fault.Kind);
        Assert.Equal(RecoveryAction.None, fault.Recovery);
        Assert.True(fault.UseFastBackoff);
    }

    [Fact]
    public void PipeTimeoutAbortsAndFlushesBeforeRetrying()
    {
        var fault = UsbFault.Classify(121); // ERROR_SEM_TIMEOUT

        Assert.Equal(FaultKind.Transient, fault.Kind);
        Assert.Equal(RecoveryAction.AbortAndFlush, fault.Recovery);
        Assert.False(fault.UseFastBackoff);
    }

    [Fact]
    public void GeneralFailureGetsTheFullPipeReset()
    {
        var fault = UsbFault.Classify(31); // ERROR_GEN_FAILURE

        Assert.Equal(FaultKind.Ambiguous, fault.Kind);
        Assert.Equal(RecoveryAction.AbortFlushReset, fault.Recovery);
    }

    [Theory]
    [InlineData(433)]  // ERROR_NO_SUCH_DEVICE
    [InlineData(1167)] // ERROR_DEVICE_NOT_CONNECTED
    public void SurpriseRemovalSkipsPipeRecoveryAndWaits(int code)
    {
        var fault = UsbFault.Classify(code);

        Assert.Equal(FaultKind.SurpriseRemoval, fault.Kind);
        Assert.Equal(RecoveryAction.WaitForArrival, fault.Recovery);
        Assert.True(fault.UseFastBackoff);
    }

    [Fact]
    public void OperationAbortedIsSwallowedWhenWeInitiatedIt()
    {
        var fault = UsbFault.Classify(995, weInitiatedAbort: true);

        Assert.Equal(FaultKind.SelfAborted, fault.Kind);
        Assert.Equal(RecoveryAction.None, fault.Recovery);
    }

    [Fact]
    public void OperationAbortedMeansDeviceGoneWhenWeDidNotInitiateIt()
    {
        var fault = UsbFault.Classify(995, weInitiatedAbort: false);

        Assert.Equal(FaultKind.SurpriseRemoval, fault.Kind);
        Assert.Equal(RecoveryAction.WaitForArrival, fault.Recovery);
    }

    [Fact]
    public void InvalidHandleIsFlaggedAsAProgrammingError()
    {
        var fault = UsbFault.Classify(6); // ERROR_INVALID_HANDLE

        Assert.Equal(FaultKind.Programming, fault.Kind);
        Assert.Equal(RecoveryAction.Reopen, fault.Recovery);
    }

    [Fact]
    public void UnknownCodesFallIntoTheAmbiguousTierAndNeverCrash()
    {
        var fault = UsbFault.Classify(1234567);

        Assert.Equal(FaultKind.Ambiguous, fault.Kind);
        Assert.Equal(RecoveryAction.AbortFlushReset, fault.Recovery);
        Assert.Contains("1234567", fault.Description);
    }

    [Fact]
    public void EveryClassificationCarriesTheOriginalCode()
    {
        foreach (int code in new[] { 2, 5, 6, 31, 32, 121, 433, 995, 1167, 999999 })
            Assert.Equal(code, UsbFault.Classify(code).Code);
    }
}
