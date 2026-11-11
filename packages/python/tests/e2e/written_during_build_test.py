"""CLI e2e: files written while the one-shot CLI builds its index reach it.

A parent and its child land after the walk, written by another table's
`on-file` command while the build ingests. Both must be in the index the
query reads, with the child's foreign key satisfied. No mocks: real console
script, real process, real filesystem, real SQLite, real `on-file` spawn.
"""

from __future__ import annotations

import json
import shutil
import subprocess


def _cli() -> str:
    dirsql = shutil.which("dirsql")
    assert dirsql is not None, (
        "`dirsql` console script not on PATH -- run `uv run maturin develop`"
    )
    return dirsql


def describe_files_written_during_the_build():
    def it_indexes_a_parent_and_child_written_while_the_build_ingests(tmp_path):
        root = tmp_path / "data-root"
        for sub in ("t", "p", "c"):
            (root / sub).mkdir(parents=True)
        (root / "t" / "go.json").write_text('{"n": 1}\n')
        parent = root / "p" / "one.json"
        child = root / "c" / "one.json"
        emit = root / "emit.sh"
        emit.write_text(
            "#!/bin/sh\n"
            f"echo '{{\"id\": 1}}' > '{parent}'\n"
            f"echo '{{\"pid\": 1}}' > '{child}'\n"
            'cat "$@"\n'
        )
        emit.chmod(0o755)
        cfg = root / ".dirsql.toml"
        cfg.write_text(
            "[[table]]\n"
            'name = "trigger"\n'
            'ddl = "CREATE TABLE trigger (n INTEGER)"\n'
            'glob = "t/*.json"\n'
            f'on-file = "{emit}"\n'
            "\n"
            "[[table]]\n"
            'name = "parent"\n'
            'ddl = "CREATE TABLE parent (id INTEGER PRIMARY KEY)"\n'
            'glob = "p/*.json"\n'
            'on-file = "cat"\n'
            "\n"
            "[[table]]\n"
            'name = "child"\n'
            'ddl = "CREATE TABLE child (pid INTEGER REFERENCES parent(id))"\n'
            'glob = "c/*.json"\n'
            'on-file = "cat"\n'
        )

        proc = subprocess.run(
            [
                _cli(),
                "query",
                "SELECT (SELECT count(*) FROM parent) AS parents, "
                "(SELECT count(*) FROM child) AS children",
                "--config",
                str(cfg),
            ],
            cwd=str(root),
            stdin=subprocess.DEVNULL,
            capture_output=True,
            text=True,
            timeout=30,
        )

        assert proc.returncode == 0, proc.stderr
        assert json.loads(proc.stdout) == [{"parents": 1, "children": 1}]
