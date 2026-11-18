"""Command-line interface, deliberately shaped like ``MystiqueUsbTool``.

Verbs: ``pipes``, ``status``, ``info``, ``init [rotation]``, ``stream [seconds]``,
``live [updates]``, plus the image path: ``image <path>`` and ``gif <path>``.
``--show-device-id`` is opt-in; redaction is the default.

The Windows tool reads sensors from DeepCool's named pipe; here ``live`` reads
Linux sysfs/procfs instead (see :mod:`mystique.telemetry`).
"""

from __future__ import annotations

import signal
import sys
import threading
import time
from dataclasses import dataclass
from typing import Dict, List, Optional, Sequence, Tuple

from .device import (
    FaultKind,
    MystiqueDevice,
    MystiqueError,
    MystiqueUsbError,
)
from .image_upload import (
    UPLOAD_HEIGHT,
    UPLOAD_WIDTH,
    ImageUploadError,
    encode_frames,
)
from .protocol import (
    DeviceStatus,
    DisplayMode,
    ProtocolError,
    TelemetryFrame,
    format_response,
    format_response_ascii,
)
from .telemetry import LinuxSensors

__all__ = ["main", "USAGE"]

USAGE = (
    "Usage: mystique "
    "[pipes|status|info|init [rotation]|stream [seconds]|live [updates]"
    "|image <path>|gif <path>] "
    "[--show-device-id] [--rotation=0..3] [--fit=contain|cover]"
)

_VERBS = ("pipes", "status", "info", "init", "stream", "live", "image", "gif")

#: One update per second. The Windows reference is paced by the sensor bridge
#: at ~1.08 s; there is no bridge here, so the cadence is ours. Edit to taste.
LIVE_INTERVAL_SECONDS = 1.0

#: Faults worth retrying in place. Anything else (device gone, permission,
#: protocol desync) exits so systemd can restart and re-run the init handshake.
_TRANSIENT_FAULTS = (FaultKind.TIMEOUT, FaultKind.STALL, FaultKind.AMBIGUOUS)
_MAX_CONSECUTIVE_TRANSIENT_FAULTS = 3


@dataclass(frozen=True)
class Invocation:
    """A parsed command line. Pure data, so it can be tested without a device."""

    operation: str
    positional: List[str]
    show_device_id: bool
    rotation: Optional[int]
    fit: str = "contain"


def main(argv: Optional[Sequence[str]] = None) -> int:
    args = list(sys.argv[1:] if argv is None else argv)

    if any(argument in ("-h", "--help") for argument in args):
        print(USAGE)
        return 0

    invocation, error = _parse_args(args)
    if error is not None:
        print(error, file=sys.stderr)
        print(USAGE, file=sys.stderr)
        return 2

    assert invocation is not None  # for type checkers; error implies no invocation

    try:
        with MystiqueDevice.open() as device:
            print(f"Device: {device.redacted_path}")
            _print_pipes(device)

            if invocation.operation == "status":
                _print_status(device.read_status())
            elif invocation.operation == "info":
                _print_info(device, invocation.show_device_id)
            elif invocation.operation == "init":
                _run_init(
                    device,
                    _read_rotation(invocation.positional, invocation.rotation),
                    invocation.show_device_id,
                )
            elif invocation.operation == "stream":
                _stream_synthetic(
                    device, _read_count(invocation.positional, 30, 1, 600)
                )
            elif invocation.operation == "live":
                _run_init(
                    device,
                    _read_rotation(invocation.positional, invocation.rotation),
                    invocation.show_device_id,
                )
                _stream_live(device, _read_count(invocation.positional, 0, 0, 86_400))
            elif invocation.operation in ("image", "gif"):
                _upload_image(device, invocation)
        return 0
    except MystiqueUsbError as exc:
        _print_usb_error(exc)
        return 1
    except ImageUploadError as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        return 1
    except (MystiqueError, ProtocolError) as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        return 1
    except OSError as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        return 1
    except KeyboardInterrupt:
        print(
            "\nInterrupted. An upload may be incomplete; run it again to replace it.",
            file=sys.stderr,
        )
        return 130


def _parse_args(args: Sequence[str]) -> Tuple[Optional[Invocation], Optional[str]]:
    """Return ``(invocation, error)``. Error is a message, not an exception.

    Form is kept compatible with ``MystiqueUsbTool``: the verb is the first
    positional, ``--show-device-id`` is a bare flag, and the second positional
    is both the init rotation and the stream/live count. ``--rotation=N`` is a
    Linux-only addition so systemd can persist a mounting rotation; it overrides
    the positional without changing the reference behaviour when unused.
    ``image``/``gif`` take a file path and an optional ``--fit``.
    """
    remaining = list(args)
    show_device_id = False
    rotation: Optional[int] = None
    fit = "contain"
    fit_seen = False

    index = 0
    while index < len(remaining):
        argument = remaining[index]

        if argument.lower() == "--show-device-id":
            show_device_id = True
            del remaining[index]
            continue

        if argument == "--fit":
            if index + 1 >= len(remaining):
                return None, "--fit requires 'contain' or 'cover'"
            fit = remaining[index + 1]
            fit_seen = True
            del remaining[index : index + 2]
            continue

        if argument.startswith("--fit="):
            fit = argument.partition("=")[2]
            fit_seen = True
            del remaining[index]
            continue

        if argument.startswith("--rotation="):
            raw = argument.partition("=")[2]
            try:
                value = int(raw)
            except ValueError:
                return None, f"invalid rotation: {raw!r} (expected 0-3)"
            if not 0 <= value <= 3:
                return None, "rotation must be 0-3 (0/90/180/270 degrees)"
            rotation = value
            del remaining[index]
            continue

        if argument.startswith("--"):
            return None, f"unknown option: {argument}"

        index += 1

    if fit not in ("contain", "cover"):
        return None, f"fit must be 'contain' or 'cover', got {fit!r}"

    positional = remaining
    operation = positional[0].lower() if positional else "pipes"
    if operation not in _VERBS:
        return None, f"unknown verb: {operation}"

    if fit_seen and operation not in ("image", "gif"):
        return None, "--fit is only valid with image or gif"
    if operation in ("image", "gif") and len(positional) < 2:
        return None, f"{operation} requires a file path"

    return Invocation(operation, positional, show_device_id, rotation, fit), None


def _read_rotation(positional: List[str], override: Optional[int] = None) -> int:
    if override is not None:
        return override
    if len(positional) < 2:
        return 0
    try:
        rotation = int(positional[1])
    except ValueError:
        return 0
    return rotation if 0 <= rotation <= 3 else 0


def _read_count(
    positional: List[str], fallback: int, minimum: int, maximum: int
) -> int:
    if len(positional) < 2:
        return fallback
    try:
        count = int(positional[1])
    except ValueError:
        return fallback
    return max(minimum, min(count, maximum))


def _print_pipes(device: MystiqueDevice) -> None:
    pipes = device.describe_pipes()
    print(f"Interface 0: {len(pipes)} endpoints")
    for pipe in pipes:
        print(
            f"  0x{pipe.address:02X} {pipe.type:<9} max-packet={pipe.max_packet_size}"
        )


def _print_status(status: DeviceStatus) -> None:
    print(
        f"Status: mode={status.mode.value} (0x{status.raw_mode:02X}), "
        f"rotation={status.rotation_degrees} degrees"
    )


def _print_info(device: MystiqueDevice, show_device_id: bool) -> None:
    # Bytes 4..41 are a per-device identifier. Redacted unless explicitly asked.
    response = device.send_info_request()
    print(f"Info hex:  {format_response(response, show_device_id)}")
    print(f"Info text: {format_response_ascii(response, show_device_id)}")
    if not show_device_id:
        print("(device identifier redacted; pass --show-device-id to reveal)")


def _run_init(device: MystiqueDevice, rotation: int, show_device_id: bool) -> None:
    print(
        "Sending reversible Machine Info initialization "
        f"(rotation {rotation * 90} degrees)..."
    )
    for step in device.enter_machine_info_mode(rotation):
        print(
            f"  command 0x{step.command:02X}: {format_response(step.response, show_device_id)}"
        )

    status = device.read_status()
    _print_status(status)
    if status.mode is not DisplayMode.MACHINE_INFO:
        print(
            f"Note: status mode byte 0x{status.raw_mode:02X} is advisory only. "
            "A real capture saw 0x00 while the panel was live in Machine Info "
            "mode, so init success is gated on the handshake, not this byte."
        )


def _upload_image(device: MystiqueDevice, invocation: Invocation) -> None:
    path = invocation.positional[1]
    rotation = invocation.rotation if invocation.rotation is not None else 0

    print(
        f"Encoding {path} for a {UPLOAD_WIDTH}x{UPLOAD_HEIGHT} panel "
        f"(fit={invocation.fit}, rotation={rotation * 90} degrees)..."
    )
    animation = encode_frames(path, invocation.fit)
    print(
        f"Uploading {len(animation)} frame(s) "
        f"({sum(len(frame) for frame in animation.frames)} JPEG bytes)..."
    )

    device.upload_images(
        animation.frames,
        animation.delays_ms,
        rotation=rotation,
        progress=lambda message: print(f"  {message}", flush=True),
    )

    print("Upload complete.")
    print(
        "The panel is now in Image mode. Returning to Machine Info mode after "
        "an upload is not reliable on this hardware; see README.md before "
        "running `mystique live` again."
    )


def _stream_synthetic(device: MystiqueDevice, seconds: int) -> None:
    print(f"Sending {seconds} one-per-second synthetic telemetry updates...")
    for second in range(seconds):
        frame = TelemetryFrame(
            cpu_temperature_c=35 + second % 25,
            cpu_usage_percent=10 + second % 80,
            memory_load_percent=40 + second % 20,
            gpu_temperature_c=45 + second % 20,
            cpu_clock_mhz=4200 + second % 8 * 100,
        )

        device.send_telemetry(frame)
        print(
            f"  {second + 1:3d}/{seconds}: CPU {frame.cpu_temperature_c}C "
            f"{frame.cpu_usage_percent}%  RAM {frame.memory_load_percent}%  "
            f"GPU {frame.gpu_temperature_c}C  {frame.cpu_clock_mhz}MHz"
        )
        time.sleep(1.0)


def _stream_live(device: MystiqueDevice, update_limit: int) -> None:
    sensors = LinuxSensors()
    stop, previous_handlers = _install_stop_handlers()

    print(f"Sensors: {sensors.describe()}")
    print(
        "Streaming live telemetry until Ctrl+C..."
        if update_limit == 0
        else f"Streaming {update_limit} live telemetry updates..."
    )

    try:
        sent = 0
        consecutive_faults = 0
        while not stop.is_set() and (update_limit == 0 or sent < update_limit):
            frame = sensors.read_frame()
            try:
                device.send_telemetry(frame)
            except MystiqueUsbError as exc:
                if exc.fault.kind not in _TRANSIENT_FAULTS:
                    raise
                consecutive_faults += 1
                print(
                    f"  transient {exc.fault.kind.value} "
                    f"({consecutive_faults}/{_MAX_CONSECUTIVE_TRANSIENT_FAULTS}): "
                    f"{exc.fault.description}",
                    file=sys.stderr,
                    flush=True,
                )
                device.recover(exc.fault.recovery)
                if consecutive_faults >= _MAX_CONSECUTIVE_TRANSIENT_FAULTS:
                    raise
                stop.wait(1.0)
                continue

            consecutive_faults = 0
            sent += 1
            print(
                f"  {sent:5d}: CPU {frame.cpu_temperature_c}C "
                f"{frame.cpu_usage_percent}% {frame.cpu_clock_mhz}MHz  "
                f"RAM {frame.memory_load_percent}%  GPU {frame.gpu_temperature_c}C",
                flush=True,
            )
            stop.wait(LIVE_INTERVAL_SECONDS)

        if stop.is_set():
            print("Stopped.")
    finally:
        _restore_handlers(previous_handlers)


def _install_stop_handlers() -> Tuple[threading.Event, Dict[int, object]]:
    """Turn SIGINT/SIGTERM into an event so the loop exits between frames."""
    stop = threading.Event()

    def request_stop(signum, frame):
        stop.set()

    previous: Dict[int, object] = {}
    for name in ("SIGINT", "SIGTERM"):
        signum = getattr(signal, name, None)
        if signum is None:
            continue
        try:
            previous[signum] = signal.getsignal(signum)
            signal.signal(signum, request_stop)
        except (ValueError, OSError):
            # Not the main thread, or the platform refuses; Ctrl+C still works.
            pass
    return stop, previous


def _restore_handlers(previous: Dict[int, object]) -> None:
    for signum, handler in previous.items():
        try:
            signal.signal(signum, handler)
        except (ValueError, OSError):
            pass


def _print_usb_error(exc: MystiqueUsbError) -> None:
    fault = exc.fault
    code = f", code {fault.code}" if fault.code is not None else ""
    print(f"ERROR ({fault.kind.value}{code}): {exc}", file=sys.stderr)

    if fault.kind is FaultKind.ACCESS_DENIED:
        print(
            "Install linux/packaging/udev/99-mystique.rules, run "
            "`sudo udevadm control --reload && sudo udevadm trigger`, add your "
            "user to plugdev, then replug the AIO. If the rule is already "
            "installed, another process (a service or the vendor app) owns the "
            "device.",
            file=sys.stderr,
        )
    elif fault.kind is FaultKind.NOT_PRESENT:
        print(
            "The AIO is not on the USB bus. Check `lsusb | grep -i 3633:0009` "
            "and the internal USB cable.",
            file=sys.stderr,
        )
    elif fault.kind in (FaultKind.TIMEOUT, FaultKind.STALL):
        print(
            "Transient USB fault. Retry; if it repeats, replug the AIO or reboot.",
            file=sys.stderr,
        )
    elif fault.kind is FaultKind.SURPRISE_REMOVAL:
        print(
            "The device left the bus. The systemd unit restarts and re-initialises it.",
            file=sys.stderr,
        )


if __name__ == "__main__":  # pragma: no cover
    raise SystemExit(main())
