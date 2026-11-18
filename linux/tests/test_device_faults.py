"""Fault-classification tests. No USB, no pyusb, no device."""

import pytest

from mystique.device import (
    FaultKind,
    MystiqueUsbError,
    RecoveryAction,
    UsbFault,
    classify,
)


@pytest.mark.parametrize("code", [-3, 13, 1])
def test_permission_errors_are_access_denied(code):
    fault = classify(code)
    assert fault.kind is FaultKind.ACCESS_DENIED
    assert fault.recovery is RecoveryAction.NONE
    assert fault.use_fast_backoff is True


@pytest.mark.parametrize("code", [-6, 16])
def test_busy_interface_is_access_denied(code):
    fault = classify(code)
    assert fault.kind is FaultKind.ACCESS_DENIED
    assert "claimed" in fault.description


@pytest.mark.parametrize("code", [-7, 110, 121, 10060])
def test_timeout_codes_all_map_to_timeout(code):
    fault = classify(code)
    assert fault.kind is FaultKind.TIMEOUT
    assert fault.recovery is RecoveryAction.ABORT_AND_FLUSH


@pytest.mark.parametrize("code", [-9, 32])
def test_stall_codes_ask_for_a_clear_halt(code):
    fault = classify(code)
    assert fault.kind is FaultKind.STALL
    assert fault.recovery is RecoveryAction.ABORT_FLUSH_RESET


@pytest.mark.parametrize("code", [-4, 19, 1167, 10054])
def test_removal_codes_are_surprise_removal(code):
    fault = classify(code)
    assert fault.kind is FaultKind.SURPRISE_REMOVAL
    assert fault.recovery is RecoveryAction.WAIT_FOR_ARRIVAL


@pytest.mark.parametrize("code", [-5, 2])
def test_missing_device_codes_are_not_present(code):
    fault = classify(code)
    assert fault.kind is FaultKind.NOT_PRESENT
    assert fault.recovery is RecoveryAction.WAIT_FOR_ARRIVAL


@pytest.mark.parametrize("code", [-2, -12, 22])
def test_invalid_requests_are_programming_errors(code):
    assert classify(code).kind is FaultKind.PROGRAMMING


def test_interrupted_is_self_aborted_only_when_we_did_it():
    assert classify(-10, we_aborted=True).kind is FaultKind.SELF_ABORTED
    assert classify(-10).kind is FaultKind.SURPRISE_REMOVAL
    assert classify(4, we_aborted=True).kind is FaultKind.SELF_ABORTED


def test_unknown_and_missing_codes_stay_ambiguous():
    assert classify(-99).kind is FaultKind.AMBIGUOUS
    assert classify(None).kind is FaultKind.AMBIGUOUS
    assert "unclassified" in classify(None).description


def test_description_can_be_overridden():
    fault = classify(13, description="could not claim interface 0")
    assert fault.kind is FaultKind.ACCESS_DENIED
    assert fault.description == "could not claim interface 0"


def test_error_string_carries_the_operation_and_code():
    error = MystiqueUsbError(
        UsbFault(FaultKind.TIMEOUT, RecoveryAction.ABORT_AND_FLUSH, "timed out", -7),
        "write 0x01",
    )
    assert "write 0x01" in str(error)
    assert "timed out" in str(error)
    assert "code -7" in str(error)
