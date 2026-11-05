"""Splice launcher-injected flags into argv."""

from __future__ import annotations

END_OF_OPTIONS = "--"


def insert_flags(argv: list[str], flags: list[str]) -> list[str]:
    """Return ``argv`` with ``flags`` placed before the first ``--``, or at the
    end when there is none; after ``--`` the core reads them as positionals."""
    if END_OF_OPTIONS not in argv:
        return [*argv, *flags]
    end = argv.index(END_OF_OPTIONS)
    return [*argv[:end], *flags, *argv[end:]]
