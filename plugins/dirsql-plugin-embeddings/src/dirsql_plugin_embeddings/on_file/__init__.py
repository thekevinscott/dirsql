"""``on-file`` console script: embed one matched file into a dirsql row.

The package barrel: ``pyproject.toml`` points the ``dirsql-embeddings-on-file``
script at ``dirsql_plugin_embeddings.on_file:on_file``, which resolves here, so
this re-export is the shipped command's public surface -- moving the callable
between modules is free, dropping it from ``__all__`` breaks the install.
"""

from .on_file import on_file

__all__ = ["on_file"]
