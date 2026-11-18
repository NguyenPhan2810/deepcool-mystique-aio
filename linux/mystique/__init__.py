"""Userspace driver for the DeepCool MYSTIQUE AIO LCD (USB 3633:0009).

The package is split so that the protocol layer can be imported and tested
without libusb or hardware::

    mystique.protocol   pure packet build/parse, no I/O
    mystique.telemetry  Linux sensor sources
    mystique.device     pyusb discovery, ownership, transfers
    mystique.cli        the six diagnostic verbs

Importing :mod:`mystique` itself pulls in the protocol layer only.
"""

from .protocol import (
    DeviceStatus,
    DisplayMode,
    InitCommand,
    TelemetryFrame,
    build_display_payload,
    build_init_sequence,
    build_packet,
    format_response,
    format_response_ascii,
    parse_status,
    validate_response,
)

__version__ = "0.1.0"

__all__ = [
    "DeviceStatus",
    "DisplayMode",
    "InitCommand",
    "TelemetryFrame",
    "build_display_payload",
    "build_init_sequence",
    "build_packet",
    "format_response",
    "format_response_ascii",
    "parse_status",
    "validate_response",
    "__version__",
]
