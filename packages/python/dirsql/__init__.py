"""dirsql - Ephemeral SQL index over a local directory.

Also available for Rust (crates.io: ``dirsql``) and TypeScript (npm: ``dirsql``).
"""

from dirsql._dirsql import (
    BLOB,
    INTEGER,
    NUMERIC,
    REAL,
    TEXT,
    RowEvent,
    Table,
    __version__,
)
from dirsql._async import DirSQL

__all__ = [
    "DirSQL",
    "Table",
    "RowEvent",
    "TEXT",
    "INTEGER",
    "REAL",
    "BLOB",
    "NUMERIC",
    "__version__",
]
