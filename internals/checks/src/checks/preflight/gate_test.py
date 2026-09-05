"""Colocated unit tests for the preflight runner's real-world seams (#781)."""

from unittest import mock

from checks.preflight.gate import default_runner, read_e2e


def describe_read_e2e():
    def it_returns_the_e2e_table_of_the_roots_config():
        with mock.patch("checks.preflight.gate.os.path.exists", return_value=True):
            with mock.patch("checks.preflight.gate.open", mock.mock_open(read_data=b"")):
                with mock.patch(
                    "checks.preflight.gate.tomllib.load",
                    return_value={"e2e": {"extra_scope": ["x"]}},
                ):
                    assert read_e2e("c.toml") == {"extra_scope": ["x"]}

    def it_returns_empty_for_a_root_with_no_config():
        assert read_e2e(None) == {}

    def it_returns_empty_when_the_config_is_absent_from_disk():
        with mock.patch("checks.preflight.gate.os.path.exists", return_value=False):
            assert read_e2e("gone.toml") == {}


def describe_default_runner():
    def it_returns_the_subprocess_return_code():
        with mock.patch(
            "checks.preflight.gate.subprocess.run",
            return_value=mock.Mock(returncode=3),
        ) as subprocess_run:
            assert default_runner(["x"], "dir") == 3
        subprocess_run.assert_called_once_with(["x"], cwd="dir", check=False)
