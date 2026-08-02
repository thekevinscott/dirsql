"""Binding-tier tests (real core, real fs) for the async DirSQL Python API."""

import asyncio
import json
import os

import pytest

from dirsql import DirSQL, Table


def describe_DirSQL_async():
    def describe_init():
        @pytest.mark.asyncio
        async def it_creates_instance_synchronously(jsonl_dir):
            db = DirSQL(
                jsonl_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE comments (id TEXT, body TEXT, author TEXT)",
                        glob="comments/**/index.jsonl",
                        on_file=lambda path: [
                            {
                                "id": os.path.basename(os.path.dirname(path)),
                                "body": row["body"],
                                "author": row["author"],
                            }
                            for line in open(path, encoding="utf-8").read().splitlines()
                            for row in [json.loads(line)]
                        ],
                    ),
                ],
            )
            assert db is not None

        @pytest.mark.asyncio
        async def it_indexes_files_after_ready(jsonl_dir):
            db = DirSQL(
                jsonl_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE comments (id TEXT, body TEXT, author TEXT)",
                        glob="comments/**/index.jsonl",
                        on_file=lambda path: [
                            {
                                "id": os.path.basename(os.path.dirname(path)),
                                "body": row["body"],
                                "author": row["author"],
                            }
                            for line in open(path, encoding="utf-8").read().splitlines()
                            for row in [json.loads(line)]
                        ],
                    ),
                ],
            )
            await db.ready()
            results = await db.query("SELECT * FROM comments")
            assert len(results) == 3

        @pytest.mark.asyncio
        async def it_skips_the_file_on_extract_error_during_ready(tmp_dir):
            os.makedirs(os.path.join(tmp_dir, "data"), exist_ok=True)
            with open(os.path.join(tmp_dir, "data", "bad.json"), "w") as f:
                f.write("not valid json")

            db = DirSQL(
                tmp_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE items (name TEXT)",
                        glob="data/*.json",
                        on_file=lambda path: [
                            json.loads(open(path, encoding="utf-8").read())
                        ],
                    ),
                ],
            )
            # Since dirsql#714 a hook that raises costs its own file, not the
            # scan: `ready()` resolves and that file contributes nothing. Which
            # file was skipped is not reachable from this binding yet (#715).
            await db.ready()
            assert await db.query("SELECT * FROM items") == []

        @pytest.mark.asyncio
        async def it_allows_multiple_ready_calls(jsonl_dir):
            db = DirSQL(
                jsonl_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE comments (id TEXT, body TEXT, author TEXT)",
                        glob="comments/**/index.jsonl",
                        on_file=lambda path: [
                            {
                                "id": os.path.basename(os.path.dirname(path)),
                                "body": row["body"],
                                "author": row["author"],
                            }
                            for line in open(path, encoding="utf-8").read().splitlines()
                            for row in [json.loads(line)]
                        ],
                    ),
                ],
            )
            await db.ready()
            await db.ready()
            results = await db.query("SELECT * FROM comments")
            assert len(results) == 3

    def describe_query():
        @pytest.mark.asyncio
        async def it_returns_results_as_list_of_dicts(jsonl_dir):
            db = DirSQL(
                jsonl_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE comments (id TEXT, body TEXT, author TEXT)",
                        glob="comments/**/index.jsonl",
                        on_file=lambda path: [
                            {
                                "id": os.path.basename(os.path.dirname(path)),
                                "body": row["body"],
                                "author": row["author"],
                            }
                            for line in open(path, encoding="utf-8").read().splitlines()
                            for row in [json.loads(line)]
                        ],
                    ),
                ],
            )
            await db.ready()
            results = await db.query(
                "SELECT author FROM comments WHERE body = 'first comment'"
            )
            assert len(results) == 1
            assert results[0]["author"] == "alice"

        @pytest.mark.asyncio
        async def it_awaits_ready_transparently_for_eager_queries(jsonl_dir):
            db = DirSQL(
                jsonl_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE comments (id TEXT, body TEXT, author TEXT)",
                        glob="comments/**/index.jsonl",
                        on_file=lambda path: [
                            {
                                "id": os.path.basename(os.path.dirname(path)),
                                "body": row["body"],
                                "author": row["author"],
                            }
                            for line in open(path, encoding="utf-8").read().splitlines()
                            for row in [json.loads(line)]
                        ],
                    ),
                ],
            )
            # No explicit ready() -- query() must await readiness internally
            # instead of failing in the pre-ready window.
            results = await db.query("SELECT * FROM comments")
            assert len(results) == 3

        @pytest.mark.asyncio
        async def it_raises_on_invalid_sql(jsonl_dir):
            db = DirSQL(
                jsonl_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE comments (id TEXT, body TEXT, author TEXT)",
                        glob="comments/**/index.jsonl",
                        on_file=lambda path: [
                            {
                                "id": os.path.basename(os.path.dirname(path)),
                                "body": row["body"],
                                "author": row["author"],
                            }
                            for line in open(path, encoding="utf-8").read().splitlines()
                            for row in [json.loads(line)]
                        ],
                    ),
                ],
            )
            await db.ready()
            with pytest.raises(Exception):
                await db.query("NOT VALID SQL")

    def describe_watch():
        @pytest.mark.asyncio
        async def it_emits_insert_events_for_new_files(tmp_dir):
            db = DirSQL(
                tmp_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE items (name TEXT)",
                        glob="**/*.json",
                        on_file=lambda path: [
                            json.loads(open(path, encoding="utf-8").read())
                        ],
                    ),
                ],
            )
            await db.ready()

            events = []

            async def collect_events():
                async for event in db.watch():
                    events.append(event)
                    if event.action == "insert":
                        break

            task = asyncio.create_task(collect_events())

            # Give the watcher time to start
            await asyncio.sleep(0.3)

            # Create a new file atomically -- write to a sibling tmp path
            # then rename into place. Without this the watcher can fire on
            # the empty file between open() and write, producing a spurious
            # error event ahead of the insert.
            final = os.path.join(tmp_dir, "new_item.json")
            tmp = final + ".tmp"
            with open(tmp, "w") as f:
                json.dump({"name": "apple"}, f)
            os.replace(tmp, final)

            try:
                await asyncio.wait_for(task, timeout=5.0)
            except TimeoutError:
                pytest.fail("Timed out waiting for watch events")

            insert = next((e for e in events if e.action == "insert"), None)
            assert insert is not None, f"no insert event in {events!r}"
            assert insert.table == "items"
            assert insert.row["name"] == "apple"

        @pytest.mark.asyncio
        async def it_sets_file_path_relative_to_root_on_events(tmp_dir):
            os.makedirs(os.path.join(tmp_dir, "nested", "dir"), exist_ok=True)

            db = DirSQL(
                tmp_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE items (name TEXT)",
                        glob="**/*.json",
                        on_file=lambda path: [
                            json.loads(open(path, encoding="utf-8").read())
                        ],
                    ),
                ],
            )
            await db.ready()

            events = []

            async def collect_events():
                async for event in db.watch():
                    if event.action != "insert":
                        continue
                    events.append(event)
                    break

            task = asyncio.create_task(collect_events())
            await asyncio.sleep(0.3)

            rel_path = os.path.join("nested", "dir", "new.json")
            final = os.path.join(tmp_dir, rel_path)
            tmp = final + ".tmp"
            with open(tmp, "w") as f:
                json.dump({"name": "relative"}, f)
            os.replace(tmp, final)

            try:
                await asyncio.wait_for(task, timeout=5.0)
            except TimeoutError:
                pytest.fail("Timed out waiting for watch events")

            assert len(events) == 1
            file_path = events[0].file_path
            assert file_path is not None
            assert not os.path.isabs(file_path), (
                f"file_path should be relative, got absolute: {file_path!r}"
            )
            assert file_path.replace("\\", "/") == rel_path.replace("\\", "/")

        @pytest.mark.asyncio
        async def it_emits_a_delete_when_a_multi_row_file_shrinks(tmp_dir):
            path = os.path.join(tmp_dir, "rows.jsonl")
            with open(path, "w") as f:
                f.writelines(
                    json.dumps({"idx": i, "name": f"row-{i}"}) + "\n" for i in range(3)
                )

            db = DirSQL(
                tmp_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE rows (idx INTEGER, name TEXT)",
                        glob="*.jsonl",
                        on_file=lambda path: [
                            json.loads(line)
                            for line in open(path, encoding="utf-8").read().splitlines()
                            if line
                        ],
                    ),
                ],
            )
            await db.ready()
            assert len(await db.query("SELECT * FROM rows")) == 3

            events = []

            async def collect_events():
                async for event in db.watch():
                    events.append(event)
                    # A shrink surfaces either as one positional delete (the
                    # dropped third row) or as a full replace (3 deletes + 2
                    # inserts); drain until we've seen enough to distinguish.
                    inserts = len([e for e in events if e.action == "insert"])
                    if len(events) >= 5 or (
                        any(e.action == "delete" for e in events) and inserts >= 2
                    ):
                        break

            task = asyncio.create_task(collect_events())
            await asyncio.sleep(0.3)

            with open(path, "w") as f:
                f.writelines(
                    json.dumps({"idx": i, "name": f"row-{i}"}) + "\n" for i in range(2)
                )

            try:
                await asyncio.wait_for(task, timeout=5.0)
            except TimeoutError:
                task.cancel()

            deleted = {
                e.row.get("name") for e in events if e.action == "delete" and e.row
            }
            assert deleted, (
                f"expected at least one delete when the file shrinks; got {events!r}"
            )
            assert "row-2" in deleted, (
                f"expected a delete for the dropped row-2; got deletes {deleted!r}"
            )

            post = await db.query("SELECT * FROM rows ORDER BY idx")
            assert [r["idx"] for r in post] == [0, 1]

        @pytest.mark.asyncio
        async def it_emits_delete_events_for_removed_files(tmp_dir):
            with open(os.path.join(tmp_dir, "doomed.json"), "w") as f:
                json.dump({"name": "doomed"}, f)

            db = DirSQL(
                tmp_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE items (name TEXT)",
                        glob="**/*.json",
                        on_file=lambda path: [
                            json.loads(open(path, encoding="utf-8").read())
                        ],
                    ),
                ],
            )
            await db.ready()

            results = await db.query("SELECT * FROM items")
            assert len(results) == 1

            events = []

            async def collect_events():
                async for event in db.watch():
                    events.append(event)
                    if len(events) >= 1:
                        break

            task = asyncio.create_task(collect_events())
            await asyncio.sleep(0.3)

            os.remove(os.path.join(tmp_dir, "doomed.json"))

            try:
                await asyncio.wait_for(task, timeout=5.0)
            except TimeoutError:
                pytest.fail("Timed out waiting for watch events")

            assert len(events) >= 1
            assert events[0].action == "delete"
            assert events[0].table == "items"
            assert events[0].row["name"] == "doomed"

            results = await db.query("SELECT * FROM items")
            assert len(results) == 0

        @pytest.mark.asyncio
        async def it_emits_update_events_for_modified_files(tmp_dir):
            with open(os.path.join(tmp_dir, "item.json"), "w") as f:
                json.dump({"name": "draft"}, f)

            db = DirSQL(
                tmp_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE items (name TEXT)",
                        glob="**/*.json",
                        on_file=lambda path: [
                            json.loads(open(path, encoding="utf-8").read())
                        ],
                    ),
                ],
            )
            await db.ready()

            events = []

            async def collect_events():
                async for event in db.watch():
                    # Mid-write the watcher can deliver a spurious error
                    # event before the real diff lands; only update / delete
                    # / insert are meaningful here.
                    if event.action not in ("update", "delete", "insert"):
                        continue
                    events.append(event)
                    if len(events) >= 1:
                        break

            task = asyncio.create_task(collect_events())
            await asyncio.sleep(0.3)

            with open(os.path.join(tmp_dir, "item.json"), "w") as f:
                json.dump({"name": "final"}, f)

            try:
                await asyncio.wait_for(task, timeout=5.0)
            except TimeoutError:
                pytest.fail("Timed out waiting for watch events")

            assert len(events) >= 1
            # Could be update or delete+insert depending on diff logic
            actions = {e.action for e in events}
            assert "update" in actions or ("delete" in actions and "insert" in actions)

        @pytest.mark.asyncio
        async def it_emits_error_events_for_bad_extract(tmp_dir):
            db = DirSQL(
                tmp_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE items (name TEXT)",
                        glob="**/*.json",
                        on_file=lambda path: [
                            json.loads(open(path, encoding="utf-8").read())
                        ],
                    ),
                ],
            )
            await db.ready()

            events = []

            async def collect_events():
                async for event in db.watch():
                    if event.action != "error":
                        continue
                    events.append(event)
                    if len(events) >= 1:
                        break

            task = asyncio.create_task(collect_events())
            await asyncio.sleep(0.3)

            with open(os.path.join(tmp_dir, "bad.json"), "w") as f:
                f.write("not json at all")

            try:
                await asyncio.wait_for(task, timeout=5.0)
            except TimeoutError:
                pytest.fail("Timed out waiting for watch events")

            assert len(events) >= 1
            assert events[0].action == "error"
            assert events[0].error is not None
            # The failing file matched the `items` table's glob; the error
            # event must carry that attribution so multi-table consumers can
            # route the error to the right handler.
            assert events[0].table == "items"

        @pytest.mark.asyncio
        async def it_updates_db_on_file_changes(tmp_dir):
            db = DirSQL(
                tmp_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE items (name TEXT)",
                        glob="**/*.json",
                        on_file=lambda path: [
                            json.loads(open(path, encoding="utf-8").read())
                        ],
                    ),
                ],
            )
            await db.ready()

            results = await db.query("SELECT * FROM items")
            assert len(results) == 0

            events = []

            async def collect_events():
                async for event in db.watch():
                    # Filter to insert events only: mid-write the watcher can
                    # deliver a spurious error/update event before the insert
                    # fires, which would race the query below.
                    if event.action != "insert":
                        continue
                    events.append(event)
                    if len(events) >= 1:
                        break

            task = asyncio.create_task(collect_events())
            await asyncio.sleep(0.3)

            with open(os.path.join(tmp_dir, "new.json"), "w") as f:
                json.dump({"name": "added"}, f)

            try:
                await asyncio.wait_for(task, timeout=5.0)
            except TimeoutError:
                pytest.fail("Timed out waiting for watch events")

            results = await db.query("SELECT * FROM items")
            assert len(results) == 1
            assert results[0]["name"] == "added"

        @pytest.mark.asyncio
        async def it_watches_without_awaiting_ready_first(tmp_dir):
            # watch() must await readiness on first iteration -- calling it
            # before an explicit `await db.ready()` must still yield events,
            # not fail on a still-None core handle.
            db = DirSQL(
                tmp_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE items (name TEXT)",
                        glob="**/*.json",
                        on_file=lambda path: [
                            json.loads(open(path, encoding="utf-8").read())
                        ],
                    ),
                ],
            )

            events = []

            async def collect_events():
                async for event in db.watch():
                    if event.action != "insert":
                        continue
                    events.append(event)
                    break

            task = asyncio.create_task(collect_events())
            await asyncio.sleep(0.3)

            final = os.path.join(tmp_dir, "new_item.json")
            tmp = final + ".tmp"
            with open(tmp, "w") as f:
                json.dump({"name": "apple"}, f)
            os.replace(tmp, final)

            try:
                await asyncio.wait_for(task, timeout=5.0)
            except TimeoutError:
                pytest.fail("Timed out waiting for watch events")

            assert len(events) == 1
            assert events[0].row["name"] == "apple"

        @pytest.mark.asyncio
        async def it_surfaces_the_real_init_error_on_watch_without_ready(tmp_dir):
            os.makedirs(os.path.join(tmp_dir, "data"), exist_ok=True)
            with open(os.path.join(tmp_dir, "data", "bad.json"), "w") as f:
                f.write("not valid json")

            db = DirSQL(
                tmp_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE items (name TEXT)",
                        glob="data/*.json",
                        on_file=lambda path: [
                            json.loads(open(path, encoding="utf-8").read())
                        ],
                    ),
                ],
            )

            with pytest.raises(Exception) as exc_info:
                async for _ in db.watch():
                    break

            # The stream must surface the real construction error, not an
            # AttributeError from a captured None handle.
            assert not isinstance(exc_info.value, AttributeError)
