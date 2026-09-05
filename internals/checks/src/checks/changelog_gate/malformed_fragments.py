"""Touched fragments whose filenames break the naming convention (#494/#496)."""

from .fragment import FRAGMENT_NAME, fragment


def malformed_fragments(changed) -> list[str]:
    """Touched fragment files whose names break the naming convention."""
    return [
        path
        for path in changed
        if (frag := fragment(path)) is not None
        and frag[1] != "README.md"
        and not FRAGMENT_NAME.fullmatch(frag[1])
    ]
