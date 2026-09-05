"""The fragments a PR adds under one package's own fragment dirs (#494/#496)."""

from .fragment import FRAGMENT_NAME, fragment


def added_fragments(added, pkg: str) -> list[str]:
    """Well-formed fragments the PR adds under ``pkg``'s own fragment dirs."""
    return [
        path
        for path in added
        if (frag := fragment(path)) is not None
        and frag[0] == pkg
        and FRAGMENT_NAME.fullmatch(frag[1])
    ]
