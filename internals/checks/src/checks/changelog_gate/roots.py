"""The top-level directories that hold independently published packages.

A package is identified throughout the check by its **root-qualified
directory** (``packages/rust``, ``plugins/dirsql-plugin-embeddings``) rather
than its bare name, so the same name under two roots stays two packages.
Nothing outside these roots -- `internals/`, `docs/`, the repo-root prose
files -- can owe a fragment.
"""

ROOTS = ("packages", "plugins")
