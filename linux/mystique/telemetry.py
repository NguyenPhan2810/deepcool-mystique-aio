"""Linux sensor sources for the Machine Info panel.

On Windows the five values come from DeepCool's ``SensorBridgeServer`` named
pipe. Linux has no such bridge, so this module reads the kernel directly:

===========  =========================================================
CPU temp     ``/sys/class/hwmon/hwmon*/`` (coretemp, k10temp, zenpower)
CPU usage    ``/proc/stat`` delta between two reads
CPU clock    ``cpufreq/scaling_cur_freq``, then ``/proc/cpuinfo``
Memory       ``/proc/meminfo`` (MemAvailable, else free+buffers+cached)
GPU temp     ``/sys/class/drm/card*/device/hwmon/*/temp1_input``,
             then ``nvidia-smi``
===========  =========================================================

Every source is best-effort. A missing sensor yields 0 rather than an exception:
the panel must keep updating even on a machine with no GPU sensor at all.

The GPU vendor preference (NVIDIA, then AMD, then anything) is a heuristic for
multi-GPU machines, not a protocol fact; the panel simply shows one GPU.
"""

from __future__ import annotations

import re
import shutil
import subprocess
import time
from pathlib import Path
from typing import Callable, Dict, List, Optional, Tuple

from .protocol import TelemetryFrame

__all__ = [
    "LinuxSensors",
    "CpuUsageSampler",
    "parse_proc_stat",
    "usage_percent",
    "parse_meminfo",
    "memory_load_percent",
]

_CPU_HWMON_NAMES = ("coretemp", "k10temp", "zenpower", "cpu_thermal")
_CPU_LABEL_PREFERENCE = ("package id 0", "tctl", "tdie")

#: How long to stop asking for ``nvidia-smi`` after it failed or was absent.
#: Without this, every frame would fork a process that cannot work.
_NVIDIA_RETRY_SECONDS = 30.0

_NVIDIA_TIMEOUT_SECONDS = 2.0

#: Sentinel meaning "look for nvidia-smi on PATH". Passing ``None`` explicitly
#: disables the fallback, which is what tests do.
_AUTO_DETECT = object()


def _read_text(path: Path) -> Optional[str]:
    try:
        return path.read_text(encoding="ascii", errors="replace")
    except OSError:
        return None


def _read_int(path: Path) -> Optional[int]:
    text = _read_text(path)
    if text is None:
        return None
    try:
        return int(text.strip())
    except ValueError:
        return None


def _read_millidegrees(path: Path) -> Optional[float]:
    value = _read_int(path)
    return None if value is None else value / 1000.0


class CpuUsageSampler:
    """CPU utilisation from two ``/proc/stat`` snapshots.

    The first call returns 0.0, not an error: it has no previous snapshot, and
    the panel should light up on the first frame rather than show nothing.
    """

    def __init__(self, stat_path: Path) -> None:
        self._stat_path = Path(stat_path)
        self._previous: Optional[Tuple[int, int]] = None

    def sample(self) -> Optional[float]:
        text = _read_text(self._stat_path)
        if text is None:
            return None
        current = parse_proc_stat(text)
        if current is None:
            return None

        previous, self._previous = self._previous, current
        if previous is None:
            return 0.0
        return usage_percent(previous, current)


def parse_proc_stat(text: str) -> Optional[Tuple[int, int]]:
    """Return ``(idle_ticks, total_ticks)`` for the aggregate ``cpu`` line.

    ``guest``/``guest_nice`` are excluded because the kernel already counts them
    inside ``user``/``nice``; summing all ten columns would double-count them.
    """
    for line in text.splitlines():
        if not line.startswith("cpu "):
            continue
        fields = line.split()[1:9]
        if len(fields) < 4:
            return None
        try:
            values = [int(field) for field in fields]
        except ValueError:
            return None
        idle = values[3] + (values[4] if len(values) > 4 else 0)
        return idle, sum(values)
    return None


def usage_percent(
    previous: Tuple[int, int], current: Tuple[int, int]
) -> Optional[float]:
    """Busy share of the ticks elapsed between two snapshots, 0-100."""
    idle_delta = current[0] - previous[0]
    total_delta = current[1] - previous[1]
    if total_delta <= 0:
        return None
    usage = 100.0 * (1.0 - idle_delta / total_delta)
    return min(100.0, max(0.0, usage))


def parse_meminfo(text: str) -> Optional[Tuple[int, int]]:
    """Return ``(total_kb, available_kb)``.

    ``MemAvailable`` is the honest number. Where it is missing (kernels before
    3.14), the classic free+buffers+cached estimate is used.
    """
    values: Dict[str, int] = {}
    for line in text.splitlines():
        name, _, rest = line.partition(":")
        parts = rest.split()
        if not parts:
            continue
        try:
            values[name.strip()] = int(parts[0])
        except ValueError:
            continue

    total = values.get("MemTotal")
    if not total:
        return None
    if "MemAvailable" in values:
        available = values["MemAvailable"]
    else:
        available = (
            values.get("MemFree", 0)
            + values.get("Buffers", 0)
            + values.get("Cached", 0)
        )
    return total, min(available, total)


def memory_load_percent(total_kb: int, available_kb: int) -> float:
    if total_kb <= 0:
        return 0.0
    return min(100.0, max(0.0, 100.0 * (total_kb - available_kb) / total_kb))


class LinuxSensors:
    """Reads the five Machine Info values from sysfs/procfs.

    ``proc_root``/``sys_root`` exist so tests can point at a fake tree. The
    remaining parameters exist so the NVIDIA fallback can be exercised without a
    GPU or without blocking on a real subprocess.
    """

    def __init__(
        self,
        proc_root: Path = Path("/proc"),
        sys_root: Path = Path("/sys"),
        *,
        nvidia_smi: Optional[str] = _AUTO_DETECT,  # type: ignore[assignment]
        runner: Optional[Callable[[List[str]], Optional[str]]] = None,
        monotonic: Callable[[], float] = time.monotonic,
        nvidia_retry_seconds: float = _NVIDIA_RETRY_SECONDS,
    ) -> None:
        self.proc_root = Path(proc_root)
        self.sys_root = Path(sys_root)
        self.nvidia_smi = (
            shutil.which("nvidia-smi") if nvidia_smi is _AUTO_DETECT else nvidia_smi
        )
        self._runner = runner or _run_nvidia_smi
        self._monotonic = monotonic
        self._nvidia_retry_seconds = nvidia_retry_seconds
        self._nvidia_blocked_until = 0.0
        self._usage = CpuUsageSampler(self.proc_root / "stat")

    # -- public ------------------------------------------------------------

    def read_frame(self) -> TelemetryFrame:
        """One frame, with every unavailable sensor represented as 0."""
        return TelemetryFrame.from_raw(
            _or_zero(self.read_cpu_temperature()),
            _or_zero(self.read_cpu_usage()),
            _or_zero(self.read_memory_load()),
            _or_zero(self.read_gpu_temperature()),
            _or_zero(self.read_cpu_clock_mhz()),
        )

    def read_cpu_temperature(self) -> Optional[float]:
        for directory in self._hwmon_directories():
            name = (_read_text(directory / "name") or "").strip().lower()
            if name not in _CPU_HWMON_NAMES:
                continue
            value = _cpu_temperature(directory)
            if value is not None:
                return value
        return None

    def read_cpu_usage(self) -> Optional[float]:
        return self._usage.sample()

    def read_cpu_clock_mhz(self) -> Optional[float]:
        for filename in ("scaling_cur_freq", "cpuinfo_cur_freq"):
            value = _read_int(
                self.sys_root / "devices/system/cpu/cpu0/cpufreq" / filename
            )
            if value:
                return value / 1000.0  # kHz -> MHz

        # cpufreq is absent under some hypervisors and on older kernels.
        text = _read_text(self.proc_root / "cpuinfo")
        if text is None:
            return None
        clocks = [
            float(match.group(1))
            for match in re.finditer(r"^cpu MHz\s*:\s*([0-9.]+)", text, re.MULTILINE)
        ]
        return max(clocks) if clocks else None

    def read_memory_load(self) -> Optional[float]:
        text = _read_text(self.proc_root / "meminfo")
        if text is None:
            return None
        parsed = parse_meminfo(text)
        if parsed is None:
            return None
        return memory_load_percent(*parsed)

    def read_gpu_temperature(self) -> Optional[float]:
        found = self._gpu_hwmon_temperature()
        if found is not None:
            return found

        # Vendor sysfs is authoritative; nvidia-smi only fills the proprietary
        # NVIDIA driver case, where no hwmon node exists.
        return self._nvidia_temperature()

    def describe(self) -> str:
        """One-line summary of which sources exist, for logs and troubleshooting."""
        cpu = self._cpu_source_name()
        gpu = self._gpu_source_name()
        clock = (
            "cpufreq"
            if (
                self.sys_root / "devices/system/cpu/cpu0/cpufreq/scaling_cur_freq"
            ).exists()
            else "cpuinfo"
            if (self.proc_root / "cpuinfo").exists()
            else "missing"
        )
        memory = "meminfo" if (self.proc_root / "meminfo").exists() else "missing"
        return f"cpu={cpu} gpu={gpu} clock={clock} memory={memory}"

    # -- CPU temperature ---------------------------------------------------

    def _hwmon_directories(self) -> List[Path]:
        return sorted((self.sys_root / "class/hwmon").glob("hwmon*"))

    def _cpu_source_name(self) -> str:
        for directory in self._hwmon_directories():
            name = (_read_text(directory / "name") or "").strip()
            if name.lower() in _CPU_HWMON_NAMES:
                return name
        return "missing"

    # -- GPU temperature ---------------------------------------------------

    def _gpu_hwmon_temperature(self) -> Optional[float]:
        candidates: List[Tuple[int, int, float]] = []

        for card in sorted((self.sys_root / "class/drm").glob("card*")):
            match = re.fullmatch(r"card(\d+)", card.name)
            if match is None:
                continue  # connector entries such as card0-DP-1
            index = int(match.group(1))
            vendor = (_read_text(card / "device/vendor") or "").strip().lower()
            priority = {"0x10de": 0, "0x1002": 1}.get(vendor, 2)

            for hwmon in sorted((card / "device/hwmon").glob("hwmon*")):
                value = _hwmon_temperature(hwmon)
                if value is not None:
                    candidates.append((priority, index, value))
                    break

        if not candidates:
            return None
        return min(candidates, key=lambda item: (item[0], item[1]))[2]

    def _gpu_source_name(self) -> str:
        for card in sorted((self.sys_root / "class/drm").glob("card*")):
            if re.fullmatch(r"card(\d+)", card.name) is None:
                continue
            vendor = (_read_text(card / "device/vendor") or "").strip().lower()
            if not list((card / "device/hwmon").glob("hwmon*")):
                continue
            return {
                "0x10de": "nvidia",
                "0x1002": "amd",
                "0x8086": "intel",
            }.get(vendor, vendor or "unknown")
        return "nvidia-smi" if self.nvidia_smi else "missing"

    def _nvidia_temperature(self) -> Optional[float]:
        if not self.nvidia_smi:
            return None
        if self._monotonic() < self._nvidia_blocked_until:
            return None

        output = self._runner(
            [
                self.nvidia_smi,
                "--query-gpu=temperature.gpu",
                "--format=csv,noheader,nounits",
            ]
        )
        if output is None:
            self._nvidia_blocked_until = self._monotonic() + self._nvidia_retry_seconds
            return None

        first_line = output.strip().splitlines()[0] if output.strip() else ""
        try:
            return float(first_line)
        except ValueError:
            self._nvidia_blocked_until = self._monotonic() + self._nvidia_retry_seconds
            return None


def _or_zero(value: Optional[float]) -> float:
    return 0.0 if value is None else value


def _cpu_temperature(directory: Path) -> Optional[float]:
    """Pick the package/Tctl sensor, not an individual core."""
    entries: List[Tuple[str, float]] = []
    for path in sorted(directory.glob("temp*_input")):
        value = _read_millidegrees(path)
        if value is None:
            continue
        label_path = path.with_name(path.name.replace("_input", "_label"))
        label = (_read_text(label_path) or "").strip().lower()
        entries.append((label, value))

    for preferred in _CPU_LABEL_PREFERENCE:
        for label, value in entries:
            if preferred in label:
                return value
    return entries[0][1] if entries else None


def _hwmon_temperature(directory: Path) -> Optional[float]:
    """temp1 is the whole-GPU sensor on amdgpu, nouveau, and i915."""
    value = _read_millidegrees(directory / "temp1_input")
    if value is not None:
        return value
    for path in sorted(directory.glob("temp*_input")):
        value = _read_millidegrees(path)
        if value is not None:
            return value
    return None


def _run_nvidia_smi(argv: List[str]) -> Optional[str]:
    try:
        result = subprocess.run(
            argv,
            capture_output=True,
            text=True,
            timeout=_NVIDIA_TIMEOUT_SECONDS,
            check=False,
        )
    except (OSError, subprocess.SubprocessError):
        return None
    if result.returncode != 0:
        return None
    return result.stdout
