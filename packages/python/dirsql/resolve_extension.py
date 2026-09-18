"""Resolve an extension entry's ``path`` to a concrete loadable file.

Resolution is an ordered probe (file-first, then package), planned by the core
(``_dirsql.plan_extension_path``) and carried out here: only locating an
installed package is host-specific.

1. **Path-looking** (contains a separator, or ends in ``.so`` / ``.dylib`` /
   ``.dll`` / ``.pyd``) -- returned as a file path: made absolute against
   ``base`` when ``resolve_relative`` is set (config-file entries), else
   verbatim (programmatic entries).
2. **Bare name** -- a same-named local file under ``base`` *shadows* the
   package; otherwise the package dir is located via
   :func:`importlib.util.find_spec` and the current platform's loadable is
   picked from inside it.
"""

from ._dirsql import plan_extension_path
from .plan_entry_path import _plan_entry_path


def resolve_extension_path(path, base, resolve_relative):
    """Resolve an extension ``path`` to a concrete file.

    ``base`` is the directory a relative path and the bare-name shadow probe
    resolve against (a config file's parent dir, or the cwd for programmatic
    entries). ``resolve_relative`` makes a relative path-looking value absolute
    against ``base`` (config-file semantics); when false it is returned verbatim
    (programmatic semantics).
    """
    return _plan_entry_path(plan_extension_path(path, base, resolve_relative))
