"""What counts as a fragment path, and what a well-formed one is named.

Fragments live inside the package they document -- ``<root>/<pkg>/changelog.d/``
and ``<root>/<pkg>/migrations.d/`` -- so they ship with that package. The
directory identifies the package, so the filename carries no package token:
just an ISO date, a kebab-case slug, and ``.md``.
"""

import re

from .roots import ROOTS

# Captures (<pkg dir>, <filename>); the trailing segment forbids nested paths.
_FRAGMENT_RE = re.compile(
    rf"((?:{'|'.join(ROOTS)})/[^/]+)/(?:changelog|migrations)\.d/([^/]+)"
)

# Template-lib fragment name: an ISO date, a kebab-case slug, and `.md`.
FRAGMENT_NAME = re.compile(r"\d{4}-\d{2}-\d{2}-[a-z0-9-]+\.md")


def fragment(path: str):
    """``(<pkg dir>, <filename>)`` if ``path`` sits directly in a package's
    changelog.d/ or migrations.d/, else ``None``."""
    match = _FRAGMENT_RE.fullmatch(path)
    return (match.group(1), match.group(2)) if match is not None else None
