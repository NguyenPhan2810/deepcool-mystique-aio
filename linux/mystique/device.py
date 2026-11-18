"""pyusb transport for the DeepCool MYSTIQUE AIO LCD.

Ownership model (see ``linux/DESIGN.md``):

* exactly one process claims interface 0 for the lifetime of its
  :class:`MystiqueDevice`;
* no kernel driver is detached, because the device is vendor-specific and has
  none bound (``re/CONTEXT.md``);
* every transfer passes an explicit timeout, so no code path can block forever;
* closing sends nothing. The panel keeps its last frame, which is the honest
  "nobody is driving me" state (same decision as the Windows service).

The module imports ``usb`` lazily so that ``mystique.protocol`` and the unit
tests work on machines without pyusb or libusb. Only :func:`open` needs it.
"""

from __future__ import annotations

import time
from dataclasses import dataclass
from enum import Enum
from typing import Callable, List, Optional, Sequence, Tuple

from .image_upload import Operation, build_upload_sequence
from .protocol import (
    DATA_PIPE_IN,
    DATA_PIPE_OUT,
    INIT_PIPE_IN,
    INIT_PIPE_OUT,
    READ_BUFFER_LENGTH,
    COMMAND_INFO,
    COMMAND_STATUS,
    COMMAND_TELEMETRY,
    DeviceStatus,
    TelemetryFrame,
    build_display_payload,
    build_init_sequence,
    build_packet,
    parse_status,
    validate_response,
)

__all__ = [
    "VENDOR_ID",
    "PRODUCT_ID",
    "INTERFACE_NUMBER",
    "TRANSFER_TIMEOUT_MS",
    "FaultKind",
    "RecoveryAction",
    "UsbFault",
    "PipeInfo",
    "InitStep",
    "MystiqueError",
    "MystiqueUsbError",
    "DeviceStateError",
    "MystiqueDevice",
    "classify",
]

VENDOR_ID = 0x3633
PRODUCT_ID = 0x0009
INTERFACE_NUMBER = 0

#: Same transfer timeout the Windows reference uses on all four pipes.
#: Bulk pipes default to infinite, which is why every call below passes it.
TRANSFER_TIMEOUT_MS = 1500

#: libusb error codes (negative), named here so the mapping is readable and so
#: this module does not need pyusb imported to classify a stored code.
_LIBUSB_ERROR_IO = -1
_LIBUSB_ERROR_INVALID_PARAM = -2
_LIBUSB_ERROR_ACCESS = -3
_LIBUSB_ERROR_NO_DEVICE = -4
_LIBUSB_ERROR_NOT_FOUND = -5
_LIBUSB_ERROR_BUSY = -6
_LIBUSB_ERROR_TIMEOUT = -7
_LIBUSB_ERROR_PIPE = -9
_LIBUSB_ERROR_INTERRUPTED = -10
_LIBUSB_ERROR_NOT_SUPPORTED = -12

# pyusb normally reports libusb's negative status, but backends can surface the
# platform errno instead (observed on Windows: 13 for EACCES). Map the common
# positive values too, plus the Win32 codes the C# reference uses, so a
# permission failure is never mislabelled as "ambiguous".
_POSIX_EPERM = 1
_POSIX_ENOENT = 2
_POSIX_EINTR = 4
_POSIX_EACCES = 13
_POSIX_EBUSY = 16
_POSIX_ENODEV = 19
_POSIX_EINVAL = 22
_POSIX_EPIPE = 32
_POSIX_ETIMEDOUT = 110
_WIN32_SEM_TIMEOUT = 121
_WIN32_DEVICE_NOT_CONNECTED = 1167
_WINSOCK_TIMEDOUT = 10060
_WINSOCK_CONNECTION_RESET = 10054


class FaultKind(Enum):
    """What a failure means, not where it happened.

    This mirrors the C# ``FaultKind`` where the meaning is the same. Two kinds
    are Linux-specific: ``ACCESS_DENIED`` (a missing udev rule or a claimed
    interface) and ``STALL`` (an endpoint halt, which libusb names directly).
    """

    NOT_PRESENT = "not-present"
    ACCESS_DENIED = "access-denied"
    TIMEOUT = "timeout"
    STALL = "stall"
    SURPRISE_REMOVAL = "surprise-removal"
    SELF_ABORTED = "self-aborted"
    AMBIGUOUS = "ambiguous"
    PROGRAMMING = "programming"


class RecoveryAction(Enum):
    """The C# recovery ladder, mapped onto what libusb can actually do."""

    NONE = "none"
    ABORT_AND_FLUSH = "abort-and-flush"
    ABORT_FLUSH_RESET = "abort-flush-reset"
    REOPEN = "reopen"
    WAIT_FOR_ARRIVAL = "wait-for-arrival"


@dataclass(frozen=True)
class UsbFault:
    """A classified failure plus the recovery it calls for."""

    kind: FaultKind
    recovery: RecoveryAction
    description: str
    code: Optional[int] = None
    use_fast_backoff: bool = False


class MystiqueError(Exception):
    """Base class for failures this driver understands."""


class MystiqueUsbError(MystiqueError):
    """A USB operation failed; ``fault`` carries the handling decision."""

    def __init__(self, fault: UsbFault, operation: str) -> None:
        self.fault = fault
        self.operation = operation
        code = f", code {fault.code}" if fault.code is not None else ""
        super().__init__(f"{operation} failed: {fault.description}{code}")


class DeviceStateError(MystiqueError):
    """The device answered but is not in the state the driver requires."""


@dataclass(frozen=True)
class PipeInfo:
    """One bulk endpoint of interface 0."""

    address: int
    type: str
    max_packet_size: int


@dataclass(frozen=True)
class InitStep:
    """One command of the init handshake and the device's reply."""

    command: int
    response: bytes


def classify(
    code: Optional[int],
    *,
    description: Optional[str] = None,
    we_aborted: bool = False,
) -> UsbFault:
    """Map a USB backend error code onto the taxonomy.

    Accepts libusb's negative status (the normal case), and the positive POSIX,
    Win32, and Winsock values some backends report instead. Unknown codes are
    treated as ambiguous, never fatal: nothing here decides to give up, it only
    decides how to recover. ``description`` can override the default text when
    the call site knows more (for example, which operation needed the interface).
    """
    if code in (_LIBUSB_ERROR_ACCESS, _POSIX_EACCES, _POSIX_EPERM):
        fault = UsbFault(
            FaultKind.ACCESS_DENIED,
            RecoveryAction.NONE,
            description
            or (
                "permission denied opening 3633:0009; install "
                "linux/packaging/udev/99-mystique.rules and add your user to "
                "plugdev (or use the uaccess rule), then replug the device"
            ),
            code,
            use_fast_backoff=True,
        )
    elif code in (_LIBUSB_ERROR_BUSY, _POSIX_EBUSY):
        fault = UsbFault(
            FaultKind.ACCESS_DENIED,
            RecoveryAction.NONE,
            description
            or "USB interface is already claimed by a kernel driver or another process",
            code,
            use_fast_backoff=True,
        )
    elif code in (
        _LIBUSB_ERROR_NO_DEVICE,
        _POSIX_ENODEV,
        _WIN32_DEVICE_NOT_CONNECTED,
        _WINSOCK_CONNECTION_RESET,
    ):
        fault = UsbFault(
            FaultKind.SURPRISE_REMOVAL,
            RecoveryAction.WAIT_FOR_ARRIVAL,
            description or "device is not on the USB bus",
            code,
            use_fast_backoff=True,
        )
    elif code in (_LIBUSB_ERROR_NOT_FOUND, _POSIX_ENOENT):
        fault = UsbFault(
            FaultKind.NOT_PRESENT,
            RecoveryAction.WAIT_FOR_ARRIVAL,
            description or "device is not on the USB bus",
            code,
            use_fast_backoff=True,
        )
    elif code in (
        _LIBUSB_ERROR_TIMEOUT,
        _POSIX_ETIMEDOUT,
        _WIN32_SEM_TIMEOUT,
        _WINSOCK_TIMEDOUT,
    ):
        fault = UsbFault(
            FaultKind.TIMEOUT,
            RecoveryAction.ABORT_AND_FLUSH,
            description or "bulk transfer timed out",
            code,
        )
    elif code in (_LIBUSB_ERROR_PIPE, _POSIX_EPIPE):
        fault = UsbFault(
            FaultKind.STALL,
            RecoveryAction.ABORT_FLUSH_RESET,
            description or "endpoint stalled (halted)",
            code,
        )
    elif code in (_LIBUSB_ERROR_INTERRUPTED, _POSIX_EINTR):
        if we_aborted:
            fault = UsbFault(
                FaultKind.SELF_ABORTED,
                RecoveryAction.NONE,
                description or "transfer cancelled by us",
                code,
                use_fast_backoff=True,
            )
        else:
            fault = UsbFault(
                FaultKind.SURPRISE_REMOVAL,
                RecoveryAction.WAIT_FOR_ARRIVAL,
                description or "transfer aborted by the USB stack",
                code,
                use_fast_backoff=True,
            )
    elif code in (
        _LIBUSB_ERROR_INVALID_PARAM,
        _LIBUSB_ERROR_NOT_SUPPORTED,
        _POSIX_EINVAL,
    ):
        fault = UsbFault(
            FaultKind.PROGRAMMING,
            RecoveryAction.REOPEN,
            description or f"invalid request for this device (code {code})",
            code,
        )
    else:
        fault = UsbFault(
            FaultKind.AMBIGUOUS,
            RecoveryAction.ABORT_FLUSH_RESET,
            description
            or (
                f"unclassified libusb error {code}"
                if code is not None
                else "unclassified USB error (no libusb status reported)"
            ),
            code,
        )
    return fault


_USB: Optional[Tuple[object, object]] = None


def _load_usb() -> Tuple[object, object]:
    """Import pyusb on first use, with an actionable failure message."""
    global _USB
    if _USB is None:
        try:
            import usb.core  # type: ignore[import-not-found]
            import usb.util  # type: ignore[import-not-found]

            _USB = (usb.core, usb.util)
        except ImportError as exc:  # pragma: no cover - depends on the machine
            raise MystiqueUsbError(
                UsbFault(
                    FaultKind.PROGRAMMING,
                    RecoveryAction.NONE,
                    "pyusb is not installed (pip install pyusb); no USB access possible",
                ),
                "load libusb backend",
            ) from exc
    return _USB


def _abandon(util: object, device: object, interface: int) -> None:
    """Release a half-opened device without masking the original failure."""
    try:
        util.release_interface(device, interface)
    except Exception:
        pass
    finally:
        util.dispose_resources(device)


class MystiqueDevice:
    """One claimed interface and its four bulk endpoints.

    Not thread-safe by design: one worker performs exchanges. The only
    concurrency in this driver is an :class:`threading.Event` in the CLI used to
    wake a sleeping loop on SIGTERM; it never touches the device.
    """

    def __init__(self, device: object, interface: int, timeout_ms: int) -> None:
        self._device = device
        self._interface = interface
        self._timeout_ms = timeout_ms
        self._closed = False

    # -- lifecycle ---------------------------------------------------------

    @classmethod
    def open(
        cls,
        *,
        timeout_ms: int = TRANSFER_TIMEOUT_MS,
        interface: int = INTERFACE_NUMBER,
        serial: Optional[str] = None,
    ) -> "MystiqueDevice":
        """Find, claim, and validate the device as one indivisible step.

        The Windows reference refuses to hand out a device without its pipe
        policy applied, because a reopen that forgets the timeout hangs forever.
        The same rule applies here: the timeout is part of the constructor and
        there is no way to obtain a device without it.
        """
        if timeout_ms <= 0:
            raise ValueError("timeout_ms must be positive; 0 means infinite in libusb")
        if not 0 <= interface <= 255:
            raise ValueError(f"interface must be 0-255, got {interface!r}")

        core, util = _load_usb()

        criteria = {"idVendor": VENDOR_ID, "idProduct": PRODUCT_ID}
        if serial is not None:
            criteria["serial_number"] = serial

        try:
            device = core.find(**criteria)
        except core.NoBackendError as exc:  # type: ignore[attr-defined]
            raise MystiqueUsbError(
                UsbFault(
                    FaultKind.PROGRAMMING,
                    RecoveryAction.NONE,
                    "no libusb backend available; install libusb-1.0 "
                    "(Debian/Ubuntu: libusb-1.0-0) and pyusb",
                ),
                "locate device",
            ) from exc
        except core.USBError as exc:  # type: ignore[attr-defined]
            raise MystiqueUsbError(_from_exception(exc), "locate device") from exc

        if device is None:
            raise MystiqueUsbError(
                UsbFault(
                    FaultKind.NOT_PRESENT,
                    RecoveryAction.WAIT_FOR_ARRIVAL,
                    f"no device {VENDOR_ID:04X}:{PRODUCT_ID:04X} on the USB bus",
                    use_fast_backoff=True,
                ),
                "locate device",
            )

        try:
            try:
                device.get_active_configuration()
            except core.USBError:  # type: ignore[attr-defined]
                # Only reachable if the device was found unconfigured, which the
                # kernel normally prevents. Never called on a configured device.
                device.set_configuration()

            # Deliberately no detach_kernel_driver: this is a vendor-specific
            # interface and no kernel driver binds to it. Taking it away from a
            # driver that did bind would be wrong; claim failure is reported.
            util.claim_interface(device, interface)
        except core.USBError as exc:  # type: ignore[attr-defined]
            util.dispose_resources(device)
            fault = _from_exception(
                exc,
                description=(
                    f"could not claim interface {interface}; on Linux this is "
                    "usually the udev rule (plugdev/uaccess) or another process"
                )
                if _error_code(exc) == _LIBUSB_ERROR_ACCESS
                else None,
            )
            raise MystiqueUsbError(fault, "claim interface") from exc

        try:
            # Belt and braces: every read/write below still passes timeout=
            # explicitly. This only protects a future call site that forgets.
            device.default_timeout = timeout_ms
            instance = cls(device, interface, timeout_ms)
            instance._validate_endpoints()
            return instance
        except core.USBError as exc:  # type: ignore[attr-defined]
            _abandon(util, device, interface)
            raise MystiqueUsbError(
                _from_exception(exc), "describe interface endpoints"
            ) from exc
        except BaseException:
            _abandon(util, device, interface)
            raise

    def close(self) -> None:
        """Release the interface and close the handle. Sends nothing to the panel."""
        if self._closed:
            return
        self._closed = True
        _, util = _load_usb()
        try:
            util.release_interface(self._device, self._interface)
        except Exception:
            # A vanished device cannot be released; closing the handle is enough.
            pass
        finally:
            util.dispose_resources(self._device)

    def __enter__(self) -> "MystiqueDevice":
        return self

    def __exit__(self, exc_type, exc, traceback) -> None:
        self.close()

    # -- description -------------------------------------------------------

    @property
    def redacted_path(self) -> str:
        """Bus/address of the device. Never the USB serial, which is identity."""
        bus = getattr(self._device, "bus", None)
        address = getattr(self._device, "address", None)
        location = (
            f"bus={bus:03d} address={address:03d}"
            if bus and address
            else "location unknown"
        )
        return f"{VENDOR_ID:04X}:{PRODUCT_ID:04X} {location}"

    def describe_pipes(self) -> List[PipeInfo]:
        """All endpoints of interface 0, in descriptor order."""
        _, util = _load_usb()
        configuration = self._device.get_active_configuration()
        try:
            interface = configuration[(self._interface, 0)]
        except KeyError as exc:
            raise DeviceStateError(
                f"interface {self._interface} alternate setting 0 is not present"
            ) from exc
        pipes = []
        for endpoint in interface:
            pipes.append(
                PipeInfo(
                    address=endpoint.bEndpointAddress,
                    type=_endpoint_type_name(util.endpoint_type(endpoint.bmAttributes)),
                    max_packet_size=endpoint.wMaxPacketSize,
                )
            )
        return pipes

    def _validate_endpoints(self) -> None:
        expected = (INIT_PIPE_OUT, INIT_PIPE_IN, DATA_PIPE_OUT, DATA_PIPE_IN)
        present = {pipe.address for pipe in self.describe_pipes()}
        missing = [address for address in expected if address not in present]
        if missing:
            formatted = ", ".join(f"0x{address:02X}" for address in missing)
            raise DeviceStateError(
                "device does not expose the MYSTIQUE endpoint pair(s) "
                f"{formatted}; found {sorted(f'0x{a:02X}' for a in present)}"
            )

    # -- protocol ----------------------------------------------------------

    def _write(self, endpoint: int, data: bytes, operation: str) -> None:
        if self._closed:
            raise DeviceStateError("device handle is closed")
        core, _ = _load_usb()

        try:
            written = self._device.write(endpoint, data, timeout=self._timeout_ms)
        except core.USBError as exc:  # type: ignore[attr-defined]
            raise MystiqueUsbError(_from_exception(exc), operation) from exc

        if written != len(data):
            raise MystiqueUsbError(
                UsbFault(
                    FaultKind.AMBIGUOUS,
                    RecoveryAction.ABORT_FLUSH_RESET,
                    f"short write: {written}/{len(data)} bytes",
                ),
                operation,
            )

    def _read(self, endpoint: int, operation: str) -> bytes:
        core, _ = _load_usb()
        try:
            data = self._device.read(
                endpoint, READ_BUFFER_LENGTH, timeout=self._timeout_ms
            )
        except core.USBError as exc:  # type: ignore[attr-defined]
            raise MystiqueUsbError(_from_exception(exc), operation) from exc
        return bytes(data)

    def exchange(
        self,
        out_endpoint: int,
        in_endpoint: int,
        command: int,
        payload: bytes = b"",
    ) -> bytes:
        """Write one 48-byte packet and read its reply, with a timeout on both."""
        packet = build_packet(command, payload)
        operation = (
            f"write 0x{out_endpoint:02X}/read 0x{in_endpoint:02X} "
            f"command 0x{command:02X}"
        )

        self._write(out_endpoint, packet, operation)
        response = self._read(in_endpoint, operation)
        validate_response(response, command)
        return response

    def execute_operation(self, operation: Operation) -> bytes:
        """Execute one pure upload step from :mod:`mystique.image_upload`.

        Frame data (header+JPEG) and the finish marker are written with no read.
        Command steps read one reply when the device sends one; the public
        implementation tolerates a missing reply, so a timeout there is not
        fatal. A stall, removal, or malformed reply still is.
        """
        label = operation.description or (
            f"write 0x{operation.endpoint:02X} ({len(operation.data)} bytes)"
        )
        self._write(operation.endpoint, operation.data, label)

        response = b""
        if operation.reply_endpoint is not None:
            try:
                response = self._read(operation.reply_endpoint, label)
            except MystiqueUsbError as exc:
                if operation.reply_optional and exc.fault.kind is FaultKind.TIMEOUT:
                    response = b""
                else:
                    raise
            if response and operation.command is not None and operation.validate_reply:
                validate_response(response, operation.command)

        if operation.settle_ms:
            time.sleep(operation.settle_ms / 1000.0)
        return response

    def upload_images(
        self,
        frames: Sequence[bytes],
        delays_ms: Sequence[int],
        *,
        rotation: int = 0,
        progress: Optional[Callable[[str], None]] = None,
    ) -> None:
        """Upload encoded JPEG frames as an animated image.

        The sequence erases stored images first (a second upload silently keeps
        the old content otherwise), sends each ``DCLd`` header and its JPEG in
        one write, commits, then replays the image-mode handshake.
        """
        for operation in build_upload_sequence(frames, delays_ms, rotation=rotation):
            if progress is not None and operation.description:
                progress(operation.description)
            self.execute_operation(operation)

    def read_status(self) -> DeviceStatus:
        response = self.exchange(DATA_PIPE_OUT, DATA_PIPE_IN, COMMAND_STATUS)
        return parse_status(response)

    def send_info_request(self) -> bytes:
        return self.exchange(INIT_PIPE_OUT, INIT_PIPE_IN, COMMAND_INFO)

    def send_telemetry(self, frame: TelemetryFrame) -> bytes:
        return self.exchange(
            DATA_PIPE_OUT,
            DATA_PIPE_IN,
            COMMAND_TELEMETRY,
            build_display_payload(frame),
        )

    def enter_machine_info_mode(self, rotation: int = 0) -> List[InitStep]:
        """Run the 13-command handshake.

        Must be re-run after every reopen: a reopened device is not in machine
        info mode, and telemetry writes are ACKed while displaying nothing.

        Success means the handshake completed without a transfer error. It is
        deliberately **not** gated on status byte 5: a real capture recorded
        mode ``0x00`` while the panel was demonstrably showing Machine Info
        (see docs/PROTOCOL_DEEPCOOL_MYSTIQUE.md in the public project). The
        status read is performed because the reference sequence does it, and
        its value is advisory; the CLI prints it.
        """
        steps = []
        for command in build_init_sequence(rotation):
            response = self.exchange(
                INIT_PIPE_OUT, INIT_PIPE_IN, command.command, command.payload
            )
            steps.append(InitStep(command.command, response))
            time.sleep(command.settle_ms / 1000.0)

        self.read_status()  # advisory; not a gate (see docstring)
        return steps

    # -- recovery ----------------------------------------------------------

    def recover(self, action: RecoveryAction) -> None:
        """Apply the recovery the fault asked for.

        ``ABORT_AND_FLUSH`` is a no-op here and that is correct: pyusb's
        synchronous API has no queued transfers to abort, and the completed
        call already returned. ``REOPEN`` and ``WAIT_FOR_ARRIVAL`` are the
        caller's job. Only ``ABORT_FLUSH_RESET`` has work to do: clear a halt.
        """
        if action is not RecoveryAction.ABORT_FLUSH_RESET or self._closed:
            return

        _, util = _load_usb()
        for address in (INIT_PIPE_OUT, INIT_PIPE_IN, DATA_PIPE_OUT, DATA_PIPE_IN):
            try:
                self._device.clear_halt(address)
            except Exception:
                # clear_halt on a pipe that is not halted is an error, not a
                # problem; the reset is best effort by design.
                pass


def _from_exception(exc: Exception, *, description: Optional[str] = None) -> UsbFault:
    code = _error_code(exc)
    if code is None and type(exc).__name__ == "USBTimeoutError":
        code = _LIBUSB_ERROR_TIMEOUT
    return classify(code, description=description)


def _error_code(exc: Exception) -> Optional[int]:
    """pyusb stores the backend status in ``errno`` for USB failures."""
    code = getattr(exc, "errno", None)
    return code if isinstance(code, int) and code != 0 else None


def _endpoint_type_name(value: int) -> str:
    # USB 2.0 endpoint transfer types, as printed by the C# tool.
    return {
        0: "Control",
        1: "Isochronous",
        2: "Bulk",
        3: "Interrupt",
    }.get(value, f"Unknown({value})")
