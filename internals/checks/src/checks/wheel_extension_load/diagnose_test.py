"""Colocated unit tests for the wheel probe's failure diagnosis (#755)."""

from unittest import mock

from checks.wheel_extension_load.diagnose import STATIC_MARKER, diagnose


def _result(returncode=0, stdout="", stderr=""):
    return mock.Mock(returncode=returncode, stdout=stdout, stderr=stderr)


def describe_diagnose():
    def static_binary_gets_the_dlopen_diagnosis():
        message = diagnose(_result(1, stdout="", stderr=f"boom: {STATIC_MARKER}"))
        assert "statically linked" in message
        assert "dirsql#755" in message
        assert STATIC_MARKER in message

    def the_marker_matches_sqlites_literal_wording():
        message = diagnose(_result(1, stderr="Error: Dynamic loading not supported"))
        assert "statically linked" in message

    def other_failures_get_a_generic_message():
        message = diagnose(_result(1, stdout="out", stderr="config missing"))
        assert message.startswith("`dirsql query` against the installed wheel failed.")
        assert "'config missing'" in message
        assert "'out'" in message
