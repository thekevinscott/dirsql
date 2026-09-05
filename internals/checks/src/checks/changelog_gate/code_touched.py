"""Does a package have source changes worth documenting? (#494/#496)"""

from .is_exempt import is_exempt


def code_touched(changed, pkg: str) -> bool:
    """True if the package has non-stub, non-test source changes."""
    prefix = f"{pkg}/"
    return any(
        path.startswith(prefix) and not is_exempt(path, pkg) for path in changed
    )
