"""Use case 4, the transcripts: many files, many rows.

A tree of JSONL session logs, each turned into message rows by a parser
named in `.dirsql.toml`; the question is how many messages each project has
per role. Native pipes the parser's JSON rows over every file into a
second process that decodes and counts them, the same hand-off dirsql pays.
dirsql runs the same parser over every file through the real launcher. No
mocks: real console script, real process, real filesystem, real parser spawn.
"""

from __future__ import annotations

import json
import shutil

import pytest

from .speed_of_light import (
    assert_speed_of_light,
    best_of,
    cli,
    dirsql_rows,
    dirsql_timeout,
    grow_until_native_takes_a_second,
    hopeless_seconds,
    startup_seconds,
    timed,
)

COLUMNS = ("project", "role", "n")
QUERY = (
    "SELECT project, role, count(*) AS n FROM messages"
    " GROUP BY project, role ORDER BY project, role"
)

CONFIG = """\
[[table]]
name    = "messages"
glob    = "projects/**/*.jsonl"
ddl     = "CREATE TABLE messages (project TEXT, kind TEXT, ts TEXT, role TEXT, text TEXT)"
on-file = "python3 messages.py"
"""

MESSAGES = """\
import json, sys

NOISE = ("<", "Another Claude session", "Caveat:", "This session is being continued")


def rows(path):
    parts = path.split("/")
    project = parts[parts.index("projects") + 1]
    kind = "subagent" if "subagents" in parts else "main"
    out = []
    for line in open(path, encoding="utf-8"):
        if not line.strip():
            continue
        try:
            entry = json.loads(line)
        except ValueError:
            continue
        if entry.get("type") not in ("user", "assistant"):
            continue
        content = (entry.get("message") or {}).get("content")
        if isinstance(content, str):
            text = content
        else:
            text = "\\n".join(b["text"] for b in (content or []) if b.get("type") == "text")
        if not text or text.startswith(NOISE):
            continue
        out.append({"project": project, "kind": kind, "ts": entry.get("timestamp"), "role": entry["type"], "text": text})
    return out


if __name__ == "__main__":
    out = []
    for path in sys.argv[1:]:
        out.extend(rows(path))
    print(json.dumps(out))
"""

COUNT = """\
import json, sys
from collections import Counter

counts = Counter()
for line in sys.stdin:
    for row in json.loads(line):
        counts[(row["project"], row["role"])] += 1
for (project, role), n in sorted(counts.items()):
    print(project, role, n, sep="\\t")
"""

NATIVE = "find projects -name '*.jsonl' -exec python3 messages.py {} + | python3 count.py"

LINE_KINDS = (
    {
        "type": "user",
        "message": {"content": "please run the tests again and show me the output"},
    },
    {
        "type": "assistant",
        "message": {
            "content": [
                {"type": "text", "text": "Running them now."},
                {"type": "tool_use", "name": "Bash"},
            ]
        },
    },
    {
        "type": "assistant",
        "message": {"content": [{"type": "tool_use", "name": "Read"}]},
    },
    {
        "type": "user",
        "message": {"content": "<local-command-stdout>ignored</local-command-stdout>"},
    },
    {"type": "progress", "data": {"step": 3}},
    {
        "type": "assistant",
        "message": {"content": [{"type": "text", "text": "Caveat: this is noise"}]},
    },
)
PADDING = " ".join(["filler"] * 60)


def build_transcripts(root, lo, hi):
    """Session files `lo` through `hi - 1`, two hundred lines each across
    eight projects, one in four under a `subagents/` folder, each line a few
    hundred bytes."""
    if lo == 0:
        root.mkdir(parents=True)
        (root / ".dirsql.toml").write_text(CONFIG)
        (root / "messages.py").write_text(MESSAGES)
        (root / "count.py").write_text(COUNT)
    for i in range(lo, hi):
        project = root / "projects" / f"project-{i % 8}"
        if i % 4 == 0:
            path = project / f"session-{i}" / "subagents" / "agent.jsonl"
        else:
            path = project / f"session-{i}.jsonl"
        path.parent.mkdir(parents=True, exist_ok=True)
        lines = []
        for j in range(200):
            entry = dict(LINE_KINDS[(i + j) % len(LINE_KINDS)])
            entry["timestamp"] = f"2026-09-{1 + j % 28:02d}T10:{j % 60:02d}:00Z"
            entry["uuid"] = f"{i}-{j}"
            entry["padding"] = PADDING
            lines.append(json.dumps(entry))
        path.write_text("\n".join(lines) + "\n")


def native_rows(proc):
    assert proc.returncode == 0, proc.stderr
    rows = []
    for line in proc.stdout.splitlines():
        project, role, n = line.split("\t")
        rows.append((project, role, int(n)))
    return rows


def describe_transcripts_speed_of_light():
    @pytest.fixture
    def root(tmp_path):
        tree = tmp_path / "transcripts"
        try:
            yield tree
        finally:
            shutil.rmtree(tree, ignore_errors=True)

    @pytest.mark.xfail(
        strict=True,
        reason="ingesting the parser's JSON rows costs more than a tenth of the parse itself",
    )
    def it_matches_native_rows_within_the_bar(root):
        startup = startup_seconds()

        def native():
            proc, seconds = timed(["sh", "-c", NATIVE], root, timeout=600)
            return native_rows(proc), seconds

        grow_until_native_takes_a_second(
            lambda lo, hi: build_transcripts(root, lo, hi),
            native,
            start=128,
            ceiling=2**16,
        )
        expected, native_seconds = best_of(native, "native")
        assert len(expected) == 16, expected

        def dirsql():
            proc, seconds = timed(
                [cli(), "query", QUERY, "-c", ".dirsql.toml"],
                root,
                timeout=dirsql_timeout(native_seconds),
            )
            return dirsql_rows(proc, COLUMNS), seconds

        actual, dirsql_seconds = best_of(
            dirsql, "dirsql", hopeless_seconds(native_seconds, startup)
        )

        assert actual == expected
        assert_speed_of_light("transcripts", native_seconds, dirsql_seconds, startup)
