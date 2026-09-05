"""Config-path extraction from the launcher's argv.

Every config flag occurrence counts -- ``-c``/``--config`` are repeatable,
and plugin discovery injects fragments as additional ``-c`` flags -- so the
scan collects them all, in argv order.
"""

from __future__ import annotations


def config_paths_from_argv(argv: list[str]) -> list[str]:
    """Every config value in argv, in order (``--config X``, ``--config=X``,
    ``-c X``, ``-c=X``, ``-cX``), or the default when none are given."""
    paths: list[str] = []
    i = 0
    while i < len(argv):
        a = argv[i]
        if a == "--config" or a == "-c":
            # A bare trailing flag (no following value) yields "".
            paths.append(next(iter(argv[i + 1 :]), ""))
            i += 2
            continue
        if a.startswith("--config="):
            paths.append(a[len("--config=") :])
        elif a.startswith("-c"):
            paths.append(a[len("-c") :].removeprefix("="))
        i += 1
    return paths or ["./.dirsql.toml"]
