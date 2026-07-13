"""Launcher-side plugin discovery: installed = active, CLI only (#363/#529).

A plugin declares ``[project.entry-points.dirsql]`` in its own package metadata,
naming its top-level module; the fragment is the ``dirsql.toml`` shipped inside
that module. This launcher discovers installed plugins (one
``importlib.metadata`` lookup), injects each fragment as an ordinary ``-c`` flag
**after** the user's own args, and -- when the user passed no ``-c`` -- adds the
hidden ``--include-default`` (#604) so the baked-in ``files`` table survives
alongside the plugins' tables (a bare injected ``-c`` would otherwise suppress
it, #602).

Appending is safe because config flags are subcommand-local (#609): the user's
own ``-c`` sits after the ``query`` subcommand (or at top level in server mode),
so the injected flags land in the **same** clap context and accumulate with it
-- the plugin configs merge after the user's, preserving user-first order. (A
config flag placed *before* a subcommand is a hard error under #609, so no
silent straddle is possible.)

Discovery is opt-out via ``--no-plugin`` / ``DIRSQL_NO_PLUGIN=1``; the
``--no-plugin`` flag is consumed here and never forwarded to the binary (which
does not know it). The compiled binary knows nothing about plugins, and the SDK
never discovers (Prettier v2->v3 lesson) -- only this CLI launcher does.
"""

from __future__ import annotations

import os
from importlib import metadata, resources

_ENTRY_POINT_GROUP = "dirsql"
_FRAGMENT_NAME = "dirsql.toml"
_NO_PLUGIN_FLAG = "--no-plugin"
_NO_PLUGIN_ENV = "DIRSQL_NO_PLUGIN"


def _user_passed_config(argv: list[str]) -> bool:
    """True when argv already names a ``-c`` / ``--config`` file -- the user's
    own config is the base, so the baked-in default is not re-added."""
    for arg in argv:
        # `--config` naturally fails `startswith("-c")` (it starts with `--`),
        # so the three clauses are disjoint: bare/attached short `-c`, long
        # `--config`, and the `--config=<value>` form.
        if arg == "--config" or arg.startswith("--config=") or arg.startswith("-c"):
            return True
    return False


def _discovery_disabled(argv: list[str]) -> bool:
    """True when discovery is opted out via the flag or the env var."""
    return _NO_PLUGIN_FLAG in argv or bool(os.environ.get(_NO_PLUGIN_ENV))


def _fragment_path(module_name: str) -> str:
    """Absolute path to a plugin module's shipped ``dirsql.toml``. Raises a
    clear error naming the plugin when the module or the fragment is missing --
    never a silent skip."""
    try:
        fragment = resources.files(module_name).joinpath(_FRAGMENT_NAME)
    except ModuleNotFoundError as exc:
        raise ValueError(
            f"dirsql plugin module {module_name!r} is not importable: {exc}"
        ) from exc
    if not fragment.is_file():
        raise ValueError(
            f"dirsql plugin {module_name!r} ships no {_FRAGMENT_NAME} fragment "
            f"(expected at {fragment})"
        )
    return str(fragment)


def _discovered_fragments() -> list[str]:
    """Fragment paths for every installed plugin, ordered by entry-point name
    (deterministic, so a running server's ``-c`` list is reproducible)."""
    entry_points = sorted(
        metadata.entry_points(group=_ENTRY_POINT_GROUP), key=lambda ep: ep.name
    )
    return [_fragment_path(ep.value) for ep in entry_points]


def with_discovered_plugins(argv: list[str]) -> list[str]:
    """Return ``argv`` with each installed plugin's fragment appended as ``-c``
    (plus ``--include-default`` when the user passed no ``-c``). ``--no-plugin``
    / ``DIRSQL_NO_PLUGIN`` skip discovery, consuming the flag. ``init`` takes no
    config, so it is left untouched. Raises if a declared plugin is missing its
    module or fragment (the launcher surfaces a clean error)."""
    if _discovery_disabled(argv):
        return [a for a in argv if a != _NO_PLUGIN_FLAG]
    if argv and argv[0] == "init":
        return argv
    fragments = _discovered_fragments()
    if not fragments:
        return argv
    injected: list[str] = []
    if not _user_passed_config(argv):
        injected.append("--include-default")
    for fragment in fragments:
        injected.append("-c")
        injected.append(fragment)
    return [*argv, *injected]
