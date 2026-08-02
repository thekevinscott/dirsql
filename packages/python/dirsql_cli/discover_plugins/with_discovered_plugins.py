"""Rewrite argv to activate installed plugins (the discovery orchestrator).

This is the public entry point of the ``discover_plugins`` package (installed =
active, CLI only; #363/#529). A plugin is an ordinary Python package that
declares ``[project.entry-points.dirsql]`` naming its top-level module and ships
a ``dirsql.toml`` fragment there; when installed alongside ``dirsql``, the
``pip``/``uvx`` launcher discovers it and injects the fragment as a ``-c`` flag
plus the hidden ``--include-default`` (#604) when the user gave no ``-c``.
Opt out via ``--no-plugin`` / ``DIRSQL_NO_PLUGIN=1``. The compiled binary knows
nothing about plugins, and the SDK never discovers -- only this CLI launcher.
The helpers each live in their own module (``user_passed_config``,
``discovery_disabled``, ``fragment_path``, ``discovered_fragments``).
"""

from __future__ import annotations

from .discovered_fragments import discovered_fragments
from .discovery_disabled import NO_PLUGIN_FLAG, discovery_disabled
from .user_passed_config import user_passed_config


def with_discovered_plugins(argv: list[str]) -> list[str]:
    """Return ``argv`` with each installed plugin's fragment appended as ``-c``
    (plus ``--include-default`` when the user passed no ``-c``). ``--no-plugin``
    / ``DIRSQL_NO_PLUGIN`` skip discovery, consuming the flag. ``init`` takes no
    config, so it is left untouched. Raises if a declared plugin is missing its
    module or fragment (the launcher surfaces a clean error).

    Appending is safe because config flags are subcommand-local (#609): the
    user's own ``-c`` sits after the ``query`` subcommand (or at top level in
    server mode), so the injected flags land in the same clap context and
    accumulate with it -- plugins merge after the user's config, preserving
    user-first order.
    """
    if discovery_disabled(argv):
        return [a for a in argv if a != NO_PLUGIN_FLAG]
    if argv and argv[0] == "init":
        return argv
    fragments = discovered_fragments()
    if not fragments:
        return argv
    injected: list[str] = []
    if not user_passed_config(argv):
        injected.append("--include-default")
    for fragment in fragments:
        injected.append("-c")
        injected.append(fragment)
    return [*argv, *injected]
