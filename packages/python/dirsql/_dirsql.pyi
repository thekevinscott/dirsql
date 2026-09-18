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

    name: str
    ddl: str
    glob: str
    on_file: Callable[[str], list[Row]]
    strict: bool

    def __init__(
        self,
        *,
        name: str,
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

class ScanFailure:
    """One file the initial scan could not index, with the hook's own error."""

    path: str
    message: str

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
        no_ignore: bool = False,
        config: list[str] | None = None,
        persist: bool = False,
        persist_path: str | PathLike[str] | None = None,
        extensions: list[ExtensionSpec] | None = None,
        suppress_config_extensions: bool = False,
    ) -> None: ...
    def query(self, sql: str) -> list[Row]: ...
    def scan_failures(self) -> list[ScanFailure]: ...
    def _start_watcher(self) -> None: ...
    def _poll_events(self, timeout_ms: int) -> list[RowEvent]: ...

class ExtensionPlanEntry:
    """One planned ``[[dirsql.extension]]`` entry. Exactly one of ``path`` and
    ``package`` is set: a ``path`` is ready to load, a ``package`` must be
    located with ``importlib`` -- unless ``shadow`` names an existing file,
    which takes precedence over the package."""

    path: str | None
    package: str | None
    shadow: str | None
    entrypoint: str | None

    @override
    def __repr__(self) -> str: ...

def run_cli(argv: list[str]) -> int: ...
def config_paths_from_argv(argv: list[str]) -> list[str]: ...
def plan_config_extensions(
    configs: list[tuple[str, str | None]],
) -> list[ExtensionPlanEntry] | None: ...
def select_loadable(name: str, dirs: list[str], candidates: list[str]) -> str: ...
def plan_extension_path(
    path: str, base: str, resolve_relative: bool
) -> ExtensionPlanEntry: ...
