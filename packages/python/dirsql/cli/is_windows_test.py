"""Unit tests for `is_windows`."""

from dirsql.cli.is_windows import is_windows


def describe_is_windows():
    def it_returns_true_for_nt():
        assert is_windows("nt") is True

    def it_returns_false_for_posix():
        assert is_windows("posix") is False

    def it_returns_false_for_java():
        assert is_windows("java") is False

    def it_returns_false_for_an_empty_string():
        assert is_windows("") is False
