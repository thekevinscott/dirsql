"""CLI e2e: the declared `[[table]] name` key through the real launcher.

A table's name is declared, never derived from `ddl`. The launcher must query
a config table under its declared `name`, exit non-zero when a `[[table]]`
entry omits `name`, and exit non-zero when the entry's `ddl` never creates
that name. No mocks: real console script, real process, real filesystem, real
SQLite, real `on-file` command spawn.
"""

from __future__ import annotations

import json
import shutil
import subprocess


def _cli() -> str:
    """Resolve the `dirsql` console script for this test env."""
    dirsql = shutil.which("dirsql")
    assert dirsql is not None, (
        "`dirsql` console script not on PATH -- run `uv run maturin develop`"
    )
    return dirsql


def _fixture(tmp_path, config: str):
    root = tmp_path / "data-root"
    (root / "data").mkdir(parents=True)
    (root / "data" / "a.json").write_text('[{"id": "one"}, {"id": "two"}]')
    cfg = root / ".dirsql.toml"
    cfg.write_text(config)
    return root, cfg


def _query(root, cfg, sql: str):
    return subprocess.run(
        [_cli(), "query", sql, "--config", str(cfg)],
        cwd=str(root),
        stdin=subprocess.DEVNULL,
        capture_output=True,
        text=True,
        timeout=30,
    )


def describe_declared_table_name():
    def it_queries_a_table_under_its_declared_name(tmp_path):
        root, cfg = _fixture(
            tmp_path,
            "[[table]]\n"
            'name = "records"\n'
            'ddl = "CREATE TABLE records (id TEXT)"\n'
            'glob = "data/*.json"\n'
            'on-file = "cat {path}"\n',
        )

        proc = _query(root, cfg, "SELECT id FROM records ORDER BY id")

        assert proc.returncode == 0, f"expected success, got {proc.stderr!r}"
        assert [row["id"] for row in json.loads(proc.stdout)] == ["one", "two"]

    def it_exits_nonzero_when_a_table_entry_has_no_name(tmp_path):
        root, cfg = _fixture(
            tmp_path,
            "[[table]]\n"
            'ddl = "CREATE TABLE records (id TEXT)"\n'
            'glob = "data/*.json"\n'
            'on-file = "cat {path}"\n',
        )

        proc = _query(root, cfg, "SELECT id FROM records")

        assert proc.returncode != 0, f"expected failure, got {proc!r}"
        assert "name" in proc.stderr and "[[table]]" in proc.stderr, (
            f"stderr must name the missing `name` key, got {proc.stderr!r}"
        )

    def it_exits_nonzero_when_the_ddl_never_creates_the_declared_name(tmp_path):
        root, cfg = _fixture(
            tmp_path,
            "[[table]]\n"
            'name = "messages"\n'
            'ddl = "CREATE TABLE records (id TEXT)"\n'
            'glob = "data/*.json"\n'
            'on-file = "cat {path}"\n',
        )

        proc = _query(root, cfg, "SELECT id FROM messages")

        assert proc.returncode != 0, f"expected failure, got {proc!r}"
        assert "table 'messages'" in proc.stderr, (
            f"stderr must carry the config-entry prefix, got {proc.stderr!r}"
        )
