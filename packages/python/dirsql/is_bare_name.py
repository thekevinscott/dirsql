"""Whether an extension ``path`` names a package rather than a file."""

import os

# Suffixes that mark a value as "already a file path", so package resolution
# is never attempted.
_LOADABLE_SUFFIXES = (".so", ".dylib", ".dll", ".pyd")


def is_bare_name(path):
    """True when ``path`` is a bare package name rather than a file path."""
    if os.sep in path or (os.altsep and os.altsep in path):
        return False
    return not path.endswith(_LOADABLE_SUFFIXES)
