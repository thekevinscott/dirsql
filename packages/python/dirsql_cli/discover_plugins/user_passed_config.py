"""Whether the user's argv already names a config file."""

from __future__ import annotations


def user_passed_config(argv: list[str]) -> bool:
    """True when argv already names a ``-c`` / ``--config`` file -- the user's
    own config is the base, so the baked-in default is not re-added."""
    for arg in argv:
        # `--config` naturally fails `startswith("-c")` (it starts with `--`),
        # so the three clauses are disjoint: bare/attached short `-c`, long
        # `--config`, and the `--config=<value>` form.
        if arg == "--config" or arg.startswith("--config=") or arg.startswith("-c"):
            return True
    return False
