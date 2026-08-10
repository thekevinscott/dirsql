"""Integration: the packaged config fragment declares embed() and sqlite-vec.

Reads the packaged ``dirsql.toml`` exactly as the launcher does and pins the
whole surface it declares: the sqlite-vec ``[[dirsql.extension]]`` entry
(unchanged) plus the ``[[dirsql.function]]`` entry for ``embed``, exactly as
specified in #801.
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
    def it_declares_exactly_the_extension_and_the_function():
        fragment = _fragment()
        assert set(fragment) == {"dirsql"}, fragment
        assert set(fragment["dirsql"]) == {"extension", "function"}, fragment[
            "dirsql"
        ]

    def it_keeps_the_sqlite_vec_extension_entry():
        (extension,) = _fragment()["dirsql"]["extension"]
        assert extension == {"path": "sqlite_vec", "entrypoint": "sqlite3_vec_init"}

    def it_declares_the_embed_function_exactly_per_spec():
        (function,) = _fragment()["dirsql"]["function"]
        assert function == {
            "name": "embed",
            "args": [1, 2],
            "command": "dirsql-plugin-embeddings worker",
            "deterministic": True,
            "timeout": "600s",
        }


def describe_console_scripts():
    def it_ships_the_worker_entry_point():
        (script,) = entry_points(
            group="console_scripts", name="dirsql-plugin-embeddings"
        )
        assert script.value == "dirsql_plugin_embeddings.cli:main"
