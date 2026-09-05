"""Resolve an extension entry's ``path`` to a concrete loadable file.

Resolution is an ordered probe (file-first, then package), so only a bare
package name reaches the package machinery:

1. **Path-looking** (contains a separator, or ends in ``.so`` / ``.dylib`` /
   ``.dll`` / ``.pyd``) -- returned as a file path: made absolute against
   ``base`` when ``resolve_relative`` is set (config-file entries), else
   verbatim (programmatic entries).
2. **Bare name** -- a same-named local file under ``base`` *shadows* the
   package; otherwise the package dir is located via
   :func:`importlib.util.find_spec` and the current platform's loadable is
   globbed from inside it.
"""

import os

from .is_bare_name import is_bare_name
from .resolve_package import _resolve_package


def resolve_extension_path(path, base, resolve_relative):
    """Resolve an extension ``path`` to a concrete file.

    ``base`` is the directory a relative path and the bare-name shadow probe
    resolve against (a config file's parent dir, or the cwd for programmatic
    entries). ``resolve_relative`` makes a relative path-looking value absolute
    against ``base`` (config-file semantics); when false it is returned verbatim
    (programmatic semantics).
    """
    if not is_bare_name(path):
        if resolve_relative and not os.path.isabs(path):
            return os.path.join(base, path)
        return path
    local = os.path.join(base, path)
    if os.path.isfile(local):
        return local
    return _resolve_package(path)
