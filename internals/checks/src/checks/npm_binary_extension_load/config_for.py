"""The scratch config that points the probe at a loadable extension library."""

from __future__ import annotations

ENTRYPOINT = "sqlite3_vec_init"


def config_for(library_path: str) -> str:
    return (
        f'[[dirsql.extension]]\npath = "{library_path}"\n'
        f'entrypoint = "{ENTRYPOINT}"\n'
    )
