"""Argument parsing tests. No device and no pyusb needed."""

import pytest

from mystique.cli import Invocation, _parse_args, _read_count, _read_rotation, main


def test_default_verb_is_pipes():
    invocation, error = _parse_args([])
    assert error is None
    assert invocation == Invocation("pipes", [], False, None)


def test_verbs_are_case_insensitive():
    invocation, _ = _parse_args(["STATUS"])
    assert invocation.operation == "status"


def test_show_device_id_is_opt_in_and_removed_from_positionals():
    invocation, error = _parse_args(["--show-device-id", "info"])
    assert error is None
    assert invocation.show_device_id is True
    assert invocation.positional == ["info"]


def test_reference_positionals_are_preserved():
    invocation, error = _parse_args(["live", "0"])
    assert error is None
    assert invocation.operation == "live"
    assert invocation.positional == ["live", "0"]
    assert invocation.rotation is None


def test_rotation_flag_overrides_without_becoming_positional():
    invocation, error = _parse_args(["--rotation=2", "live", "0"])
    assert error is None
    assert invocation.rotation == 2
    assert invocation.positional == ["live", "0"]


@pytest.mark.parametrize(
    "argument", ["--rotation=4", "--rotation=-1", "--rotation=x", "--rotation="]
)
def test_invalid_rotation_flag_is_an_error(argument):
    invocation, error = _parse_args([argument, "pipes"])
    assert invocation is None
    assert "rotation" in error


def test_unknown_flag_is_an_error():
    invocation, error = _parse_args(["--bogus", "pipes"])
    assert invocation is None
    assert "--bogus" in error


def test_unknown_verb_is_an_error():
    invocation, error = _parse_args(["frobnicate"])
    assert invocation is None
    assert "frobnicate" in error


def test_fit_accepts_space_and_equals_forms():
    invocation, error = _parse_args(["--fit", "cover", "image", "a.png"])
    assert error is None
    assert invocation.fit == "cover"
    assert invocation.positional == ["image", "a.png"]

    invocation, error = _parse_args(["--fit=contain", "gif", "b.gif"])
    assert error is None
    assert invocation.fit == "contain"
    assert invocation.positional == ["gif", "b.gif"]


def test_fit_defaults_to_contain():
    invocation, error = _parse_args(["image", "a.png"])
    assert error is None
    assert invocation.fit == "contain"


def test_invalid_fit_is_an_error():
    invocation, error = _parse_args(["--fit=stretch", "image", "a.png"])
    assert invocation is None
    assert "fit" in error


def test_fit_with_a_non_image_verb_is_an_error():
    invocation, error = _parse_args(["--fit=cover", "status"])
    assert invocation is None
    assert "--fit" in error


def test_image_without_a_path_is_an_error():
    invocation, error = _parse_args(["image"])
    assert invocation is None
    assert "file path" in error


def test_image_rotation_flag_is_kept_for_the_upload_handshake():
    invocation, error = _parse_args(["--rotation=3", "image", "a.png"])
    assert error is None
    assert invocation.rotation == 3
    assert invocation.positional == ["image", "a.png"]


def test_help_returns_zero_without_touching_a_device(capsys):
    assert main(["--help"]) == 0
    assert "Usage: mystique" in capsys.readouterr().out


def test_bad_arguments_return_two(capsys):
    assert main(["--rotation=9", "pipes"]) == 2
    assert "rotation" in capsys.readouterr().err


def test_rotation_positional_falls_back_to_zero():
    assert _read_rotation(["init"]) == 0
    assert _read_rotation(["init", "2"]) == 2
    assert _read_rotation(["init", "9"]) == 0
    assert _read_rotation(["init", "not-a-number"]) == 0
    assert _read_rotation(["init", "9"], override=3) == 3


def test_count_positional_is_clamped_like_the_windows_tool():
    assert _read_count(["stream"], 30, 1, 600) == 30
    assert _read_count(["stream", "5"], 30, 1, 600) == 5
    assert _read_count(["stream", "0"], 30, 1, 600) == 1
    assert _read_count(["stream", "9999"], 30, 1, 600) == 600
    assert _read_count(["live", "0"], 0, 0, 86_400) == 0
    assert _read_count(["live", "x"], 0, 0, 86_400) == 0
