"""Whether a `[[package]]`'s publish globs would republish a no-op (#944).

The carve-out form the messages point at, and why a leading-`!` negation is not
it, are explained in `decide.py`.
"""

from __future__ import annotations

from .carve_out import carve_out
from .decide import NON_SHIPPING_DIRS, NON_SHIPPING_FILES, negations
from .subtree_globs import subtree_globs
from .subtree_roots import subtree_roots


def glob_problems(packages) -> list[str]:
    """One message per `[[package]]` glob entry that would republish a no-op."""
    problems = []
    for package in packages:
        name = package.get("name", "<unnamed>")
        globs = package.get("globs", [])
        for glob in negations(globs):
            problems.append(
                f'{name}: glob "{glob}" is a leading-`!` negation, which '
                f"putitoutthere does not support -- its matcher ORs the globs "
                f"together, so the negation subtracts nothing and instead "
                f"matches every path outside its own subtree. Write the "
                f"exclusion as an extglob inside a positive pattern: "
                f'"{carve_out("<root>")[1]}".'
            )
        for root in subtree_roots(globs):
            found = subtree_globs(globs, root)
            expected = carve_out(root)
            if found != sorted(expected):
                problems.append(
                    f"{name}: publish globs for {root} are "
                    f"{', '.join(found)} -- expected {', '.join(expected)}. "
                    f"A subtree glob that does not carve out "
                    f"{', '.join((*NON_SHIPPING_DIRS, *NON_SHIPPING_FILES))} "
                    f"republishes the package for changes that never reach its "
                    f"artifact. Replace the entries above in putitoutthere.toml."
                )
    return problems
