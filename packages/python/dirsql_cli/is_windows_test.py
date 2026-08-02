"""Unit tests for `is_windows`."""

import os
from unittest.mock import patch

from dirsql_cli.is_windows import is_windows


def describe_is_windows():
    def it_returns_true_when_os_name_is_nt():
        with patch.object(os, "name", "nt"):
            assert is_windows() is True

    def it_returns_false_when_os_name_is_posix():
        with patch.object(os, "name", "posix"):
            assert is_windows() is False

    def it_returns_false_when_os_name_is_anything_else():
        with patch.object(os, "name", "java"):
            assert is_windows() is False
