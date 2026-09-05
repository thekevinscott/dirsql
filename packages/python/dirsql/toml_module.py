"""The TOML parser this interpreter reads ``.dirsql.toml`` files with."""

import sys
from importlib import import_module


def _load_toml_module():
    """Return the TOML parser module for the running interpreter.

    ``tomllib`` is stdlib only on 3.11+; on 3.10 the ``tomli`` backport it was
    derived from (a version-gated dependency) provides the same surface.
    Imported by name so a unit test can exercise both arms on any
    interpreter -- a literal ``import tomllib`` is unreachable on 3.10 no
    matter what ``sys.version_info`` claims.
    """
    if sys.version_info >= (3, 11):
        return import_module("tomllib")
    return import_module("tomli")


_toml = _load_toml_module()
