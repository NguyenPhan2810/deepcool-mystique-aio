"""Tests for the Linux sensor sources.

They run against a fake /proc and /sys tree, so they pass on Windows and on any
Linux machine regardless of its actual sensors.
"""

from pathlib import Path

import pytest

from mystique.telemetry import (
    CpuUsageSampler,
    LinuxSensors,
    memory_load_percent,
    parse_meminfo,
    parse_proc_stat,
    usage_percent,
)


def _write(root: Path, relative: str, text: str) -> Path:
    path = root / relative
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="ascii")
    return path


@pytest.fixture
def machine(tmp_path):
    proc = tmp_path / "proc"
    sys_root = tmp_path / "sys"

    _write(proc, "stat", "cpu  100 0 100 800 0 0 0 0 0 0\ncpu0 50 0 50 400 0 0 0 0\n")
    _write(
        proc, "meminfo", "MemTotal:       16000000 kB\nMemAvailable:    8000000 kB\n"
    )
    _write(proc, "cpuinfo", "processor\t: 0\ncpu MHz\t\t: 4600.000\n")

    _write(sys_root, "class/hwmon/hwmon0/name", "coretemp\n")
    _write(sys_root, "class/hwmon/hwmon0/temp1_label", "Package id 0\n")
    _write(sys_root, "class/hwmon/hwmon0/temp1_input", "55000\n")
    _write(sys_root, "class/hwmon/hwmon0/temp2_label", "Core 0\n")
    _write(sys_root, "class/hwmon/hwmon0/temp2_input", "70000\n")

    _write(sys_root, "class/drm/card0/device/vendor", "0x1002\n")
    _write(sys_root, "class/drm/card0/device/hwmon/hwmon1/temp1_input", "48000\n")

    _write(
        sys_root,
        "devices/system/cpu/cpu0/cpufreq/scaling_cur_freq",
        "4600000\n",
    )
    return proc, sys_root


def test_parse_proc_stat_returns_idle_and_total_ticks():
    idle, total = parse_proc_stat("cpu  100 0 100 800 20 0 0 0 0 0\n")
    assert idle == 820  # idle + iowait
    assert total == 1020


def test_parse_proc_stat_ignores_per_cpu_lines():
    assert parse_proc_stat("cpu0 1 2 3 4\n") is None
    assert parse_proc_stat("intr 1 2 3\n") is None


def test_parse_proc_stat_rejects_garbage():
    assert parse_proc_stat("cpu  a b c d\n") is None
    assert parse_proc_stat("") is None


def test_usage_percent_from_two_snapshots():
    # +800 total, +600 idle -> 25% busy
    assert usage_percent((800, 1000), (1400, 1800)) == pytest.approx(25.0)


def test_usage_percent_clamps_and_handles_no_elapsed_ticks():
    assert usage_percent((10, 100), (20, 100)) is None
    assert usage_percent((0, 100), (0, 200)) == 100.0
    assert usage_percent((0, 100), (200, 200)) == 0.0


def test_sampler_returns_zero_on_first_read_then_a_delta(tmp_path):
    stat = tmp_path / "stat"
    stat.write_text("cpu  100 0 100 800 0 0 0 0\n", encoding="ascii")
    sampler = CpuUsageSampler(stat)

    assert sampler.sample() == 0.0

    stat.write_text("cpu  200 0 200 1400 0 0 0 0\n", encoding="ascii")
    assert sampler.sample() == pytest.approx(25.0)


def test_sampler_returns_none_when_stat_is_missing(tmp_path):
    assert CpuUsageSampler(tmp_path / "absent").sample() is None


def test_parse_meminfo_prefers_mem_available():
    total, available = parse_meminfo(
        "MemTotal: 1000 kB\nMemFree: 100 kB\nMemAvailable: 400 kB\n"
    )
    assert (total, available) == (1000, 400)


def test_parse_meminfo_falls_back_to_free_buffers_cached():
    total, available = parse_meminfo(
        "MemTotal: 1000 kB\nMemFree: 100 kB\nBuffers: 50 kB\nCached: 250 kB\n"
    )
    assert (total, available) == (1000, 400)


def test_parse_meminfo_without_total_is_missing():
    assert parse_meminfo("MemFree: 100 kB\n") is None


def test_memory_load_percent():
    assert memory_load_percent(1000, 500) == pytest.approx(50.0)
    assert memory_load_percent(0, 0) == 0.0


def test_read_frame_composes_every_source(machine):
    proc, sys_root = machine
    sensors = LinuxSensors(proc, sys_root, nvidia_smi=None)

    frame = sensors.read_frame()

    assert frame.cpu_temperature_c == 55  # Package id 0, not Core 0
    assert frame.cpu_usage_percent == 0  # first /proc/stat sample
    assert frame.memory_load_percent == 50
    assert frame.gpu_temperature_c == 48
    assert frame.cpu_clock_mhz == 4600


def test_cpu_temperature_prefers_k10temp_tctl(tmp_path):
    sys_root = tmp_path / "sys"
    _write(sys_root, "class/hwmon/hwmon0/name", "k10temp\n")
    _write(sys_root, "class/hwmon/hwmon0/temp1_label", "Tctl\n")
    _write(sys_root, "class/hwmon/hwmon0/temp1_input", "61000\n")
    _write(sys_root, "class/hwmon/hwmon0/temp2_label", "Tccd1\n")
    _write(sys_root, "class/hwmon/hwmon0/temp2_input", "50000\n")

    sensors = LinuxSensors(tmp_path / "proc", sys_root, nvidia_smi=None)

    assert sensors.read_cpu_temperature() == 61


def test_gpu_prefers_discrete_vendor_over_the_integrated_one(tmp_path):
    sys_root = tmp_path / "sys"
    _write(sys_root, "class/drm/card0/device/vendor", "0x8086\n")
    _write(sys_root, "class/drm/card0/device/hwmon/hwmon0/temp1_input", "40000\n")
    _write(sys_root, "class/drm/card1/device/vendor", "0x10de\n")
    _write(sys_root, "class/drm/card1/device/hwmon/hwmon1/temp1_input", "61000\n")

    sensors = LinuxSensors(tmp_path / "proc", sys_root, nvidia_smi=None)

    assert sensors.read_gpu_temperature() == 61


def test_gpu_uses_nvidia_smi_when_there_is_no_hwmon(tmp_path):
    sensors = LinuxSensors(
        tmp_path / "proc",
        tmp_path / "sys",
        nvidia_smi="/usr/bin/nvidia-smi",
        runner=lambda argv: "61\n",
    )
    assert sensors.read_gpu_temperature() == 61


def test_nvidia_smi_failure_is_cached_for_a_while(tmp_path):
    clock = iter([0.0, 0.0, 10.0])
    calls = []

    def runner(argv):
        calls.append(argv)
        return None

    sensors = LinuxSensors(
        tmp_path / "proc",
        tmp_path / "sys",
        nvidia_smi="/usr/bin/nvidia-smi",
        runner=runner,
        monotonic=lambda: next(clock),
        nvidia_retry_seconds=30.0,
    )

    assert sensors.read_gpu_temperature() is None
    assert sensors.read_gpu_temperature() is None
    assert len(calls) == 1


def test_cpu_clock_falls_back_to_proc_cpuinfo(tmp_path):
    proc = tmp_path / "proc"
    _write(proc, "cpuinfo", "cpu MHz\t\t: 3600.000\ncpu MHz\t\t: 4000.000\n")
    sensors = LinuxSensors(proc, tmp_path / "sys", nvidia_smi=None)

    assert sensors.read_cpu_clock_mhz() == 4000


def test_missing_sensors_produce_a_valid_all_zero_frame(tmp_path):
    sensors = LinuxSensors(tmp_path / "proc", tmp_path / "sys", nvidia_smi=None)

    frame = sensors.read_frame()

    assert frame == type(frame)(0, 0, 0, 0, 0)
    assert "missing" in sensors.describe()


def test_a_malformed_sensor_does_not_stop_the_frame(machine):
    proc, sys_root = machine
    _write(proc, "meminfo", "not a meminfo file\n")
    _write(sys_root, "class/drm/card0/device/hwmon/hwmon1/temp1_input", "garbage\n")

    frame = LinuxSensors(proc, sys_root, nvidia_smi=None).read_frame()

    assert frame.cpu_temperature_c == 55
    assert frame.memory_load_percent == 0
    assert frame.gpu_temperature_c == 0


def test_describe_names_the_sources(machine):
    proc, sys_root = machine
    description = LinuxSensors(proc, sys_root, nvidia_smi=None).describe()

    assert "cpu=coretemp" in description
    assert "gpu=amd" in description
    assert "clock=cpufreq" in description
    assert "memory=meminfo" in description
