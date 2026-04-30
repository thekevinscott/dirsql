"""dirsql - Ephemeral SQL index over a local directory.

Also available for Rust (crates.io: ``dirsql``) and TypeScript (npm: ``dirsql``).
"""

from dirsql._dirsql import Table, RowEvent, __version__
from dirsql._async import DirSQL

__all__ = ["DirSQL", "Table", "RowEvent", "__version__"]
