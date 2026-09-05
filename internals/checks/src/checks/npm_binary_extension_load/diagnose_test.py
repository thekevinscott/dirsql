"""Colocated unit tests for the npm bundled-cli probe's failure diagnosis (#762)."""

from unittest import mock

from checks.npm_binary_extension_load.diagnose import STATIC_MARKER, diagnose


def _result(returncode=0, stdout="", stderr=""):
    return mock.Mock(returncode=returncode, stdout=stdout, stderr=stderr)


def describe_diagnose():
    def static_binary_gets_the_dlopen_diagnosis():
        message = diagnose(_result(1, stdout="", stderr=f"boom: {STATIC_MARKER}"))
        assert "statically linked" in message
        assert "dirsql#762" in message
        assert "putitoutthere#605" in message
        assert STATIC_MARKER in message

    def the_marker_matches_sqlites_literal_wording():
        message = diagnose(_result(1, stderr="Error: Dynamic loading not supported"))
        assert "statically linked" in message

    def other_failures_get_a_generic_message():
        message = diagnose(_result(1, stdout="out", stderr="bad flag"))
        assert message.startswith("`dirsql query` against the bundled binary failed.")
        assert "'bad flag'" in message
        assert "'out'" in message
