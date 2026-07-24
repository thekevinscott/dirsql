"""CLI e2e: installed plugins are auto-activated by the Python launcher.

Drives the real launcher (`dirsql.cli.main:main`) + bundled binary with a
fixture plugin *installed* (a staged dist declaring `[project.entry-points.dirsql]`
on the launcher's `sys.path`). No mocks. "Installed = active" (#363): the
launcher discovers the plugin, injects its `dirsql.toml` fragment as an ordinary
`-c` flag, and adds the hidden `--include-default` (#604) when the user passed no
`-c` so the baked-in `records` table survives alongside the plugin's tables.

Discovery is CLI-only and opt-out via `--no-plugin` / `DIRSQL_NO_PLUGIN=1`.
"""

import json
import os
import shutil
import subprocess
import sys

import pytest

_REPO_ROOT = os.path.abspath(
    os.path.join(os.path.dirname(__file__), "..", "..", "..", "..")
)
_RELEASE = os.path.join(_REPO_ROOT, "target", "release", "dirsql")
_DEBUG = os.path.join(_REPO_ROOT, "target", "debug", "dirsql")
_BINARY = _RELEASE if os.path.exists(_RELEASE) else _DEBUG

_FIXTURES = os.path.join(os.path.dirname(__file__), "fixtures")

# Where the launcher's `binary_path()` looks: `<dirsql package>/_binary/dirsql`.
import dirsql as _dirsql_pkg  # noqa: E402

_BINARY_STAGE_DIR = os.path.join(os.path.dirname(_dirsql_pkg.__file__), "_binary")

# An `on-file` hook emitting `path` and `basename`, derived from the file
# path (`{path}`) relative to the scan root (`{root}`).
_HOOK_PATH_BASENAME = r"""on-file = '''sh -c 'rel=${1#"$2"/}; base=${1##*/}; printf "[{\"path\":\"%s\",\"basename\":\"%s\"}]" "$rel" "$base"' sh {path} {root}'''"""


def _stage_plugin(site_dir):
    """Stage the fixture plugin as a discoverable dist under `site_dir`:
    the importable module plus a `.dist-info` carrying the `dirsql` entry point,
    exactly what `importlib.metadata` scans `sys.path` for."""
    shutil.copytree(
        os.path.join(_FIXTURES, "dirsql_plugin_fixture"),
        os.path.join(site_dir, "dirsql_plugin_fixture"),
        ignore=shutil.ignore_patterns("__pycache__"),
    )
    dist = os.path.join(site_dir, "dirsql_plugin_fixture-0.0.0.dist-info")
    os.makedirs(dist)
    with open(os.path.join(dist, "METADATA"), "w") as f:
        f.write("Metadata-Version: 2.1\nName: dirsql-plugin-fixture\nVersion: 0.0.0\n")
    with open(os.path.join(dist, "entry_points.txt"), "w") as f:
        f.write("[dirsql]\nfixture = dirsql_plugin_fixture\n")


def _run(site_dir, args, cwd, env_extra=None):
    """Run the launcher one-shot with the fixture plugin on `sys.path`."""
    env = {
        **os.environ,
        "PYTHONPATH": site_dir + os.pathsep + os.environ.get("PYTHONPATH", ""),
    }
    if env_extra:
        env.update(env_extra)
    return subprocess.run(
        [
            sys.executable,
            "-c",
            "import sys; from dirsql.cli.main import main; sys.exit(main())",
            *args,
        ],
        cwd=str(cwd),
        capture_output=True,
        text=True,
        env=env,
    )


def _basenames(out):
    rows = json.loads(out.stdout)
    return [r.get("basename") for r in rows]


def describe_plugin_discovery():
    @pytest.fixture
    def staged(tmp_path):
        assert os.path.exists(_BINARY), (
            f"dirsql binary not built at {_BINARY}; "
            "run `cargo build -p dirsql --features cli` first"
        )
        os.makedirs(_BINARY_STAGE_DIR, exist_ok=True)
        staged_binary = os.path.join(_BINARY_STAGE_DIR, "dirsql")
        shutil.copy(_BINARY, staged_binary)
        os.chmod(staged_binary, 0o755)

        site_dir = tmp_path / "site"
        site_dir.mkdir()
        _stage_plugin(str(site_dir))

        data = tmp_path / "data"
        data.mkdir()
        (data / "hello.md").write_text("# hi\n")
        try:
            yield str(site_dir), data
        finally:
            shutil.rmtree(_BINARY_STAGE_DIR, ignore_errors=True)

    def it_activates_an_installed_plugins_table_with_no_config(staged):
        site_dir, data = staged
        # The plugin's `notes` table is queryable with zero config edits...
        notes = _run(site_dir, ["query", "SELECT basename FROM notes"], data)
        assert notes.returncode == 0, f"stdout={notes.stdout!r} stderr={notes.stderr!r}"
        assert _basenames(notes) == ["hello.md"]

    def it_keeps_the_baked_in_records_table_alongside_the_plugin(staged):
        site_dir, data = staged
        # ...and the baked-in default `records` table is still served (proves the
        # launcher added `--include-default`, not a bare `-c` that would suppress it).
        records = _run(site_dir, ["query", "SELECT COUNT(*) AS n FROM records"], data)
        assert records.returncode == 0, (
            f"stdout={records.stdout!r} stderr={records.stderr!r}"
        )

    def it_skips_discovery_under_the_no_plugin_flag(staged):
        site_dir, data = staged
        out = _run(site_dir, ["--no-plugin", "query", "SELECT * FROM notes"], data)
        assert out.returncode != 0, (
            f"--no-plugin must not load the plugin: {out.stdout!r}"
        )
        assert "notes" in out.stderr

    def it_skips_discovery_under_the_env_var(staged):
        site_dir, data = staged
        out = _run(
            site_dir,
            ["query", "SELECT * FROM notes"],
            data,
            env_extra={"DIRSQL_NO_PLUGIN": "1"},
        )
        assert out.returncode != 0, (
            f"DIRSQL_NO_PLUGIN=1 must not load the plugin: {out.stdout!r}"
        )
        assert "notes" in out.stderr

    def it_composes_a_user_config_with_the_plugin(staged):
        site_dir, data = staged
        cfg = data / "user.toml"
        cfg.write_text(
            "[[table]]\n"
            'ddl = "CREATE TABLE posts (path TEXT, basename TEXT)"\n'
            'glob = "*.md"\n'
            f"{_HOOK_PATH_BASENAME}\n"
        )
        # A user `-c` is the base; the plugin is appended -> both tables load.
        # Config flags are subcommand-local (#609), so `-c` follows the SQL.
        posts = _run(
            site_dir, ["query", "SELECT basename FROM posts", "-c", "user.toml"], data
        )
        assert posts.returncode == 0, f"stdout={posts.stdout!r} stderr={posts.stderr!r}"
        notes = _run(
            site_dir, ["query", "SELECT basename FROM notes", "-c", "user.toml"], data
        )
        assert notes.returncode == 0, f"stdout={notes.stdout!r} stderr={notes.stderr!r}"

    def it_errors_when_a_plugin_table_collides_with_a_user_config(staged):
        site_dir, data = staged
        cfg = data / "dup.toml"
        cfg.write_text(
            '[[table]]\nddl = "CREATE TABLE notes (path TEXT)"\nglob = "*.md"\n'
        )
        out = _run(site_dir, ["query", "SELECT 1", "-c", "dup.toml"], data)
        assert out.returncode != 0, (
            f"a duplicate `notes` table must conflict: {out.stdout!r}"
        )
        assert "notes" in out.stderr
