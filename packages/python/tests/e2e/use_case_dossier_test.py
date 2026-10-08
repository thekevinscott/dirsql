"""Use case 1, the dossier: three selections over a note tree, UNIONed.

A standing set of background files, the five newest dailies by name, and the
ten most recently modified markdown files outside the dailies, in one ranked
list. Native is three `find` runs with `-printf`, each sorted and cut, then
one final sort. dirsql is the article's query through the real launcher.
No mocks: real console script, real process, real filesystem.
"""

from __future__ import annotations

import os
import shutil
import time

import pytest

from .speed_of_light import (
    Native,
    agreed_rows,
    assert_speed_of_light,
    baseline,
    cli,
    dirsql_rows,
    grow_until_native_takes_a_second,
    paired,
    shell_natives,
    startup_seconds,
    timed,
)

DAY = 86400
WINDOW_DAYS = 14
STANDING = ("situation.md", "projects.md", "profile.md")
COLUMNS = ("rank", "kind", "path", "mtime")

QUERY = """
WITH standing AS (
  SELECT path, mtime FROM './**'
    WHERE dir = 'planner/background'
      AND basename IN ('situation.md', 'projects.md', 'profile.md')
),
dailies AS (
  SELECT path, mtime FROM './**'
    WHERE path LIKE 'vault/Dailies/Daily %'
    ORDER BY basename DESC LIMIT 5
),
recent AS (
  SELECT path, mtime FROM './**'
    WHERE ext = 'md'
      AND path NOT LIKE 'vault/Dailies/%'
      AND mtime > strftime('%s', 'now') - 14 * 86400
    ORDER BY mtime DESC LIMIT 10
)
SELECT 1 AS rank, 'standing' AS kind, path, mtime FROM standing
UNION ALL SELECT 2, 'daily', path, mtime FROM dailies
UNION ALL SELECT 3, 'recent', path, mtime FROM recent
ORDER BY rank, mtime DESC, path
"""

NATIVE = [
    Native(
        "find+sort",
        ("bash",),
        r"""
set -e
export LC_ALL=C
cutoff=$(( $(date +%s) - 14 * 86400 ))
{
  find planner/background -maxdepth 1 \
    \( -name situation.md -o -name projects.md -o -name profile.md \) \
    -printf '1\tstanding\t%p\t%T@\n'
  find vault/Dailies -name 'Daily *' -printf '%f\t%p\t%T@\n' \
    | sort -t "$(printf '\t')" -k1,1r | head -5 \
    | awk -F '\t' -v OFS='\t' '{ print 2, "daily", $2, $3 }'
  find . -path ./vault/Dailies -prune -o -name '*.md' -newermt "@$cutoff" \
    -printf '%T@\t%P\n' \
    | sort -rn | head -10 \
    | awk -F '\t' -v OFS='\t' '{ print 3, "recent", $2, $1 }'
} | sort -t "$(printf '\t')" -k1,1n -k4,4rn -k3,3
""",
    )
]


def _write(path, mtime, body="# note\nbody\n"):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(body)
    os.utime(path, (mtime, mtime))


def build_tree(root, lo, hi):
    """Bulk notes `lo` through `hi - 1`: the first thousand inside the recency
    window, the rest older, every tenth one a `.txt` the ext filter must
    drop. Mtimes are distinct integers nowhere near the window's edge."""
    now = int(time.time())
    if lo == 0:
        for name in STANDING:
            _write(root / "planner" / "background" / name, now - 70 * DAY)
        for day in range(1, 41):
            stamp = now - day * DAY
            date = time.strftime("%Y.%m.%d", time.gmtime(stamp))
            _write(root / "vault" / "Dailies" / f"Daily {date}.md", stamp)
    for i in range(lo, hi):
        ext = "txt" if i % 10 == 0 else "md"
        mtime = now - 3600 - i * 600 if i < 1000 else now - 15 * DAY - i * 7
        _write(
            root / "planner" / "internal" / "notes" / str(i % 1000) / f"note-{i}.{ext}",
            mtime,
        )


def native_rows(proc):
    rows = []
    for line in proc.stdout.splitlines():
        rank, kind, path, mtime = line.split("\t")
        rows.append((int(rank), kind, path, int(float(mtime))))
    return rows


def describe_dossier_speed_of_light():
    @pytest.fixture
    def root(tmp_path):
        tree = tmp_path / "notes"
        try:
            yield tree
        finally:
            shutil.rmtree(tree, ignore_errors=True)

    def it_matches_native_rows_within_the_bar(root):
        startup = startup_seconds()

        natives = shell_natives(root, NATIVE, native_rows, shell="bash")

        grow_until_native_takes_a_second(
            lambda lo, hi: build_tree(root, lo, hi),
            baseline(natives),
            start=50_000,
            ceiling=2**21,
        )

        def dirsql(timeout):
            proc, seconds = timed(
                [cli(), "query", QUERY],
                root,
                timeout=timeout,
            )
            return dirsql_rows(proc, COLUMNS), seconds

        result = paired(natives, dirsql, startup)
        expected, actual = agreed_rows(result), result.dirsql_rows
        assert len(expected) == 18, expected

        assert actual == expected
        assert_speed_of_light("dossier", result, startup)
