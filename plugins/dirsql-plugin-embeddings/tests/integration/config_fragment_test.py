"""Integration: installing the plugin is inert beyond loading sqlite-vec.

Reads the packaged ``dirsql.toml`` exactly as the launcher does and pins the
whole surface it declares: one sqlite-vec extension entry and nothing else --
no declared table, no query hooks, no console scripts on PATH.
"""

import sys
from importlib import resources
from importlib.metadata import entry_points

if sys.version_info >= (3, 11):
    import tomllib
else:
    import tomli as tomllib


def _fragment():
    text = (
        resources.files("dirsql_plugin_embeddings")
        .joinpath("dirsql.toml")
        .read_text(encoding="utf-8")
    )
    return tomllib.loads(text)


def describe_config_fragment():
    def it_declares_only_extension_loading():
        fragment = _fragment()
        assert set(fragment) == {"dirsql"}, fragment
        assert set(fragment["dirsql"]) == {"extension"}, fragment["dirsql"]

    def it_keeps_the_sqlite_vec_extension_entry():
        (extension,) = _fragment()["dirsql"]["extension"]
        assert extension == {"path": "sqlite_vec", "entrypoint": "sqlite3_vec_init"}


def describe_installed_surface():
    def it_ships_no_console_scripts():
        stale = {
            entry.name
            for entry in entry_points(group="console_scripts")
            if entry.name.startswith("dirsql-embeddings-")
        }
        assert not stale, stale
