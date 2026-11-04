"""Binding-tier tests: a declared table reads exactly what is on disk.

Every query runs the moment the write before it returns, and must read what
a ``DirSQL`` built fresh over the same tree reads.
"""

import asyncio
import os
import random
import shutil
import threading

import pytest

from dirsql import DirSQL, Table

ROUNDS = 2000


def _notes(root):
    roots = [os.path.realpath(root), root]

    def on_file(path):
        rel = path
        for base in roots:
            if path.startswith(base + os.sep):
                rel = path[len(base) + 1 :]
                break
        rel = rel.replace(os.sep, "/")
        with open(path, encoding="utf-8") as f:
            text = f.read()
        return [{"file": rel, "line": line} for line in text.splitlines() if line]

    return Table(
        name="notes",
        ddl="CREATE TABLE notes (file TEXT, line TEXT)",
        glob="**/*.txt",
        on_file=on_file,
    )


async def _open(root):
    db = DirSQL(root, tables=[_notes(root)])
    await db.ready()
    return db


def _write(root, rel, content):
    path = os.path.join(root, rel)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8", newline="") as f:
        f.write(content)


def _save_atomically(root, rel, content):
    _write(root, rel + ".stage", content)
    os.replace(os.path.join(root, rel + ".stage"), os.path.join(root, rel))


def _times(root, rel):
    st = os.stat(os.path.join(root, rel))
    return (st.st_atime_ns, st.st_mtime_ns)


def _restore_times(root, rel, times):
    os.utime(os.path.join(root, rel), ns=times)


async def _lines(db, rel):
    rows = await db.query(f"SELECT line FROM notes WHERE file = '{rel}' ORDER BY line")
    return [row["line"] for row in rows]


async def _snapshot(db):
    rows = await db.query("SELECT file, line FROM notes ORDER BY file, line")
    return [(row["file"], row["line"]) for row in rows]


async def _assert_matches_fresh_scan(db, root, context):
    fresh = await _open(root)
    assert await _snapshot(db) == await _snapshot(fresh), (
        f"the long-lived instance disagrees with a fresh scan after {context}"
    )


DIRS = ["", "a/", "a/b/", "c/"]
NAMES = ["n0", "n1", "n2", "n3"]
EXTS = ["txt", "json", "stage"]
IGNORES = ["n0.txt\n", "a/\n", "*.txt\n", "c/n1.*\n"]


def _content(rng):
    return "".join(
        f"line-{rng.randrange(10_000):04}\n" for _ in range(rng.randrange(1, 4))
    )


def _random_file(rng):
    return f"{rng.choice(DIRS)}{rng.choice(NAMES)}.{rng.choice(EXTS)}"


def _existing(root):
    return [
        f"{d}{n}.{e}"
        for d in DIRS
        for n in NAMES
        for e in EXTS
        if os.path.isfile(os.path.join(root, f"{d}{n}.{e}"))
    ]


def _step(root, rng):
    existing = _existing(root)
    op = rng.randrange(9)
    if op == 1 and existing:
        rel = rng.choice(existing)
        times = _times(root, rel)
        with open(os.path.join(root, rel), encoding="utf-8") as f:
            old = f.read()
        flipped = "".join(str((int(c) + 1) % 10) if c.isdigit() else c for c in old)
        _write(root, rel, flipped)
        _restore_times(root, rel, times)
        return f"same-size edit of {rel} with its times restored"
    if op == 2:
        rel = _random_file(rng)
        _save_atomically(root, rel, _content(rng))
        return f"atomic save of {rel}"
    if op == 3 and existing:
        rel = rng.choice(existing)
        os.remove(os.path.join(root, rel))
        return f"delete {rel}"
    if op == 4 and existing:
        src = rng.choice(existing)
        dst = _random_file(rng)
        if src == dst:
            return "nothing"
        os.makedirs(os.path.dirname(os.path.join(root, dst)), exist_ok=True)
        os.replace(os.path.join(root, src), os.path.join(root, dst))
        return f"rename {src} to {dst}"
    if op == 5:
        new = f"{rng.choice(DIRS)}new{rng.randrange(3)}"
        for name in NAMES[:2]:
            _write(root, f"{new}/{name}.{rng.choice(EXTS)}", _content(rng))
        return f"populate new directory {new}"
    if op == 6:
        gone = rng.choice(DIRS[1:]).rstrip("/")
        shutil.rmtree(os.path.join(root, gone), ignore_errors=True)
        return f"remove directory {gone}"
    if op == 7:
        if rng.randrange(3) == 0:
            if os.path.exists(os.path.join(root, ".gitignore")):
                os.remove(os.path.join(root, ".gitignore"))
            return "remove .gitignore"
        rule = rng.choice(IGNORES)
        _write(root, ".gitignore", rule)
        return f"set .gitignore to {rule!r}"
    rel = _random_file(rng)
    _write(root, rel, _content(rng))
    return f"write {rel}"


def _seed_tree(root, rng):
    os.makedirs(os.path.join(root, ".git"))
    for _ in range(8):
        _write(root, _random_file(rng), _content(rng))


def describe_a_query_right_after_a_write():
    @pytest.mark.asyncio
    async def it_reads_every_create(tmp_dir):
        db = await _open(tmp_dir)
        for i in range(ROUNDS):
            _write(tmp_dir, f"h/{i}.txt", f"c{i}")
            assert await _lines(db, f"h/{i}.txt") == [f"c{i}"], f"round {i}"
            os.remove(os.path.join(tmp_dir, f"h/{i}.txt"))

    @pytest.mark.asyncio
    async def it_reads_every_edit(tmp_dir):
        _write(tmp_dir, "e.txt", "start")
        db = await _open(tmp_dir)
        for i in range(ROUNDS):
            _write(tmp_dir, "e.txt", f"e{i}")
            assert await _lines(db, "e.txt") == [f"e{i}"], f"round {i}"

    @pytest.mark.asyncio
    async def it_reads_every_delete(tmp_dir):
        db = await _open(tmp_dir)
        for i in range(ROUNDS):
            _write(tmp_dir, "d.txt", f"d{i}")
            assert await _lines(db, "d.txt") == [f"d{i}"], f"round {i}"
            os.remove(os.path.join(tmp_dir, "d.txt"))
            assert await _lines(db, "d.txt") == [], f"round {i}"

    @pytest.mark.asyncio
    async def it_reads_every_rename_into_and_out_of_the_glob(tmp_dir):
        db = await _open(tmp_dir)
        staged = os.path.join(tmp_dir, "r.stage")
        final = os.path.join(tmp_dir, "r.txt")
        for i in range(ROUNDS):
            _write(tmp_dir, "r.stage", f"r{i}")
            os.replace(staged, final)
            assert await _lines(db, "r.txt") == [f"r{i}"], f"round {i}"
            os.replace(final, staged)
            assert await _lines(db, "r.txt") == [], f"round {i}"

    @pytest.mark.asyncio
    async def it_reads_every_atomic_save(tmp_dir):
        _write(tmp_dir, "a.txt", "start")
        db = await _open(tmp_dir)
        for i in range(ROUNDS):
            _save_atomically(tmp_dir, "a.txt", f"a{i}")
            assert await _lines(db, "a.txt") == [f"a{i}"], f"round {i}"

    @pytest.mark.asyncio
    async def it_reads_every_same_size_edit(tmp_dir):
        _write(tmp_dir, "s.txt", "s------")
        db = await _open(tmp_dir)
        for i in range(ROUNDS):
            _write(tmp_dir, "s.txt", f"s{i:06}")
            assert await _lines(db, "s.txt") == [f"s{i:06}"], f"round {i}"

    @pytest.mark.asyncio
    async def it_reads_every_same_size_edit_with_its_times_restored(tmp_dir):
        _write(tmp_dir, "t.txt", "t------")
        times = _times(tmp_dir, "t.txt")
        db = await _open(tmp_dir)
        for i in range(ROUNDS):
            _write(tmp_dir, "t.txt", f"t{i:06}")
            _restore_times(tmp_dir, "t.txt", times)
            assert await _lines(db, "t.txt") == [f"t{i:06}"], f"round {i}"


def describe_a_long_lived_instance():
    @pytest.mark.asyncio
    async def it_reads_what_a_fresh_scan_reads_after_random_operations(tmp_dir):
        for seed in range(1, 4):
            root = os.path.join(tmp_dir, f"seed{seed}")
            os.makedirs(root)
            rng = random.Random(seed)
            _seed_tree(root, rng)
            db = await _open(root)
            for op in range(100):
                done = _step(root, rng)
                await _assert_matches_fresh_scan(
                    db, root, f"seed {seed} op {op}: {done}"
                )

    @pytest.mark.asyncio
    async def it_reads_what_a_fresh_scan_reads_across_idle_gaps_and_bursts(tmp_dir):
        rng = random.Random(99)
        _seed_tree(tmp_dir, rng)
        db = await _open(tmp_dir)
        for cycle in range(10):
            await asyncio.sleep(0.25 * (cycle % 4))
            for op in range(20):
                done = _step(tmp_dir, rng)
                await _assert_matches_fresh_scan(
                    db, tmp_dir, f"cycle {cycle} op {op}: {done}"
                )


def describe_each_way_a_change_could_be_missed():
    @pytest.mark.asyncio
    async def it_reads_an_edit_whose_times_are_put_back(tmp_dir):
        _write(tmp_dir, "touched.txt", "before")
        times = _times(tmp_dir, "touched.txt")
        db = await _open(tmp_dir)
        assert await _lines(db, "touched.txt") == ["before"]
        _write(tmp_dir, "touched.txt", "after!")
        _restore_times(tmp_dir, "touched.txt", times)
        assert await _lines(db, "touched.txt") == ["after!"]

    @pytest.mark.asyncio
    async def it_reads_a_file_deleted_and_recreated_under_the_same_name(tmp_dir):
        _write(tmp_dir, "again.txt", "first")
        db = await _open(tmp_dir)
        assert await _lines(db, "again.txt") == ["first"]
        os.remove(os.path.join(tmp_dir, "again.txt"))
        _write(tmp_dir, "again.txt", "secnd")
        assert await _lines(db, "again.txt") == ["secnd"]

    @pytest.mark.asyncio
    async def it_reads_a_new_and_a_deleted_subdirectory(tmp_dir):
        db = await _open(tmp_dir)
        _write(tmp_dir, "sub/deep/one.txt", "one")
        assert await _lines(db, "sub/deep/one.txt") == ["one"]
        shutil.rmtree(os.path.join(tmp_dir, "sub"))
        assert await _lines(db, "sub/deep/one.txt") == []

    @pytest.mark.asyncio
    async def it_reads_a_gitignore_change(tmp_dir):
        os.makedirs(os.path.join(tmp_dir, ".git"))
        _write(tmp_dir, "hidden.txt", "hidden")
        db = await _open(tmp_dir)
        assert await _lines(db, "hidden.txt") == ["hidden"]
        _write(tmp_dir, ".gitignore", "hidden.txt\n")
        assert await _lines(db, "hidden.txt") == []
        os.remove(os.path.join(tmp_dir, ".gitignore"))
        assert await _lines(db, "hidden.txt") == ["hidden"]

    @pytest.mark.asyncio
    async def it_reads_a_retargeted_symlink(tmp_dir):
        _write(tmp_dir, "targets/one.dat", "one")
        _write(tmp_dir, "targets/two.dat", "two")
        link = os.path.join(tmp_dir, "link.txt")
        os.symlink(os.path.join(tmp_dir, "targets/one.dat"), link)
        db = await _open(tmp_dir)
        assert await _lines(db, "link.txt") == ["one"]
        os.remove(link)
        os.symlink(os.path.join(tmp_dir, "targets/two.dat"), link)
        assert await _lines(db, "link.txt") == ["two"]

    @pytest.mark.asyncio
    async def it_never_reads_a_file_torn_while_it_is_rewritten_and_ends_exact(tmp_dir):
        def version(k):
            return f"v{k:06}\n" * 50

        _write(tmp_dir, "busy.txt", version(0))
        db = await _open(tmp_dir)
        done = threading.Event()

        def writer():
            for k in range(1, 301):
                _save_atomically(tmp_dir, "busy.txt", version(k))
            done.set()

        thread = threading.Thread(target=writer)
        thread.start()
        while not done.is_set():
            lines = await _lines(db, "busy.txt")
            assert len(lines) == 50 and len(set(lines)) == 1, lines
        thread.join()
        assert await _lines(db, "busy.txt") == ["v000300"] * 50


def describe_watch_consumers():
    @pytest.mark.asyncio
    async def it_streams_each_change_a_query_read_first_once(tmp_dir):
        db = await _open(tmp_dir)
        stream = db.watch()
        files = 100
        for i in range(files):
            _write(tmp_dir, f"w/{i}.txt", f"w{i}")
            assert await _lines(db, f"w/{i}.txt") == [f"w{i}"], f"file {i}"
        counts = {}
        while True:
            try:
                event = await asyncio.wait_for(anext(stream), timeout=2.0)
            except TimeoutError:
                break
            assert event.action == "insert", event
            counts[event.row["line"]] = counts.get(event.row["line"], 0) + 1
        assert counts == {f"w{i}": 1 for i in range(files)}
