"""Pure decision logic for the changelog-gate check (#494/#496).

Mirrors template-lib's reference gate (#566) in structure, adapted to dirsql's
**per-package colocation**: fragments live inside the package they document --
``<root>/<pkg>/changelog.d/`` and ``<root>/<pkg>/migrations.d/`` -- so they ship
with that package. Two top-level roots hold independently published packages:
``packages/`` (the three SDKs) and ``plugins/``. A PR that changes non-test
source under one of them must add a fragment under that package's own fragment
folders, named ``YYYY-MM-DD-<slug>.md`` (the directory identifies the package,
so no package token in the filename). The ``skip-changelog:`` commit-body line
is the bypass.

A package is identified throughout by its **root-qualified directory**
(``packages/rust``, ``plugins/dirsql-plugin-embeddings``) rather than its bare
name, so the same name under two roots stays two packages.
"""

import re

from .roots import ROOTS

# A `skip-changelog:` line anywhere in a commit body bypasses the gate. Scanned
# over raw bodies (not git's trailer parser), so it need not be a formal
# trailer -- this sidesteps the blank-line-splits-the-trailer footgun.
_SKIP_TRAILER = re.compile(r"(?im)^skip-changelog:")


def has_skip_trailer(commit_messages: str) -> bool:
    """True if any commit-body line starts with ``skip-changelog:``."""
    return bool(_SKIP_TRAILER.search(commit_messages))


def changed_packages(changed) -> list[str]:
    """Unique, sorted package dirs (``<root>/<name>``) the paths touch."""
    pkgs = {
        f"{parts[0]}/{parts[1]}"
        for path in changed
        if len(parts := path.split("/")) >= 2 and parts[0] in ROOTS
    }
    return sorted(pkgs)
