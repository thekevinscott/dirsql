"""Type stubs for the native PyO3 extension module.

Mirrors the surface defined in ``packages/python/src/lib.rs``. Hand-written
because pyo3-stub-gen would otherwise demand a build-time hook that the
maturin / putitoutthere release pipeline does not yet run.

Whenever ``src/lib.rs`` adds, renames, or removes a ``#[pyclass]``,
``#[pymethods]``, or module-level binding, this file MUST be updated in the
same PR -- and ``PARITY.md`` is the canonical reminder.
"""

from collections.abc import Callable
from os import PathLike
from typing import Any, TypedDict

from typing_extensions import NotRequired, override

__version__: str

Row = dict[str, Any]

class ExtensionSpec(TypedDict):
    """A SQLite extension to load at startup: a shared-library ``path`` and an
    optional ``entrypoint`` init-symbol override. Mirrors a
    ``[[dirsql.extension]]`` config entry."""

    path: str
    entrypoint: NotRequired[str]

class Table:
    """A table definition. Construct via keyword arguments only."""

    ddl: str
    glob: str
    strict: bool

    def __init__(
        self,
        *,
        ddl: str,
        glob: str,
        on_file: Callable[[str], list[Row]],
        strict: bool = False,
    ) -> None: ...

class RowEvent:
    """A row event produced by the watch loop."""

    table: str | None
    action: str
    row: Row | None
    old_row: Row | None
    error: str | None
    file_path: str | None

    @override
    def __repr__(self) -> str: ...

class DirSQL:
    """Synchronous binding class. ``dirsql._async.DirSQL`` wraps it."""

    def __init__(
        self,
        root: str | None = None,
        *,
        tables: list[Table] | None = None,
        ignore: list[str] | None = None,
        config: list[str] | None = None,
        persist: bool = False,
        persist_path: str | PathLike[str] | None = None,
        extensions: list[ExtensionSpec] | None = None,
        suppress_config_extensions: bool = False,
    ) -> None: ...
    def query(self, sql: str) -> list[Row]: ...
    def _start_watcher(self) -> None: ...
    def _poll_events(self, timeout_ms: int) -> list[RowEvent]: ...
