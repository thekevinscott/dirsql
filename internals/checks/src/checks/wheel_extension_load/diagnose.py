"""The human-readable diagnosis for a failed wheel extension-load probe (#755)."""

from __future__ import annotations

STATIC_MARKER = "Dynamic loading not supported"


def diagnose(result) -> str:
    output = f"stdout={result.stdout!r}\nstderr={result.stderr!r}"
    if STATIC_MARKER in result.stderr:
        return (
            f"the bundled `dirsql` binary in this wheel cannot load SQLite "
            f"extensions ({STATIC_MARKER!r}): it is statically linked "
            "(static-pie), and a static binary cannot dlopen. The release "
            "pipeline (putitoutthere `_matrix.yml`, #381) compiles the pypi "
            "bundle_cli binary against the musl-mapped triple; the fix is to "
            "build the wheel's bundled binary against the declared "
            "*-linux-gnu triple (the wheel's manylinux tag already gates the "
            "glibc floor), not to change this probe. See dirsql#755.\n"
            f"{output}"
        )
    return f"`dirsql query` against the installed wheel failed.\n{output}"
