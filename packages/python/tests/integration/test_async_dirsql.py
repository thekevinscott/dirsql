"""Integration tests for the async DirSQL Python API."""

import asyncio
import json
import os

import pytest

from dirsql import DirSQL, Table


def describe_DirSQL_async():
    def describe_init():
        @pytest.mark.asyncio
        async def it_creates_instance_synchronously(jsonl_dir):
            """DirSQL constructor is sync and returns immediately."""
            db = DirSQL(
                jsonl_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE comments (id TEXT, body TEXT, author TEXT)",
                        glob="comments/**/index.jsonl",
                        extract=lambda path, content: [
                            {
                                "id": os.path.basename(os.path.dirname(path)),
                                "body": row["body"],
                                "author": row["author"],
                            }
                            for line in content.splitlines()
                            for row in [json.loads(line)]
                        ],
                    ),
                ],
            )
            assert db is not None

        @pytest.mark.asyncio
        async def it_indexes_files_after_ready(jsonl_dir):
            """Data is available after awaiting ready()."""
            db = DirSQL(
                jsonl_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE comments (id TEXT, body TEXT, author TEXT)",
                        glob="comments/**/index.jsonl",
                        extract=lambda path, content: [
                            {
                                "id": os.path.basename(os.path.dirname(path)),
                                "body": row["body"],
                                "author": row["author"],
                            }
                            for line in content.splitlines()
                            for row in [json.loads(line)]
                        ],
                    ),
                ],
            )
            await db.ready()
            results = await db.query("SELECT * FROM comments")
            assert len(results) == 3

        @pytest.mark.asyncio
        async def it_raises_on_extract_error_during_ready(tmp_dir):
            """Extract lambda errors during ready() raise exceptions."""
            os.makedirs(os.path.join(tmp_dir, "data"), exist_ok=True)
            with open(os.path.join(tmp_dir, "data", "bad.json"), "w") as f:
                f.write("not valid json")

            db = DirSQL(
                tmp_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE items (name TEXT)",
                        glob="data/*.json",
                        extract=lambda path, content: [json.loads(content)],
                    ),
                ],
            )
            with pytest.raises(Exception):
                await db.ready()

        @pytest.mark.asyncio
        async def it_allows_multiple_ready_calls(jsonl_dir):
            """Calling ready() multiple times is safe and idempotent."""
            db = DirSQL(
                jsonl_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE comments (id TEXT, body TEXT, author TEXT)",
                        glob="comments/**/index.jsonl",
                        extract=lambda path, content: [
                            {
                                "id": os.path.basename(os.path.dirname(path)),
                                "body": row["body"],
                                "author": row["author"],
                            }
                            for line in content.splitlines()
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
            """Async query returns list of dicts with column names."""
            db = DirSQL(
                jsonl_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE comments (id TEXT, body TEXT, author TEXT)",
                        glob="comments/**/index.jsonl",
                        extract=lambda path, content: [
                            {
                                "id": os.path.basename(os.path.dirname(path)),
                                "body": row["body"],
                                "author": row["author"],
                            }
                            for line in content.splitlines()
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
        async def it_raises_on_invalid_sql(jsonl_dir):
            """Invalid SQL raises an exception."""
            db = DirSQL(
                jsonl_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE comments (id TEXT, body TEXT, author TEXT)",
                        glob="comments/**/index.jsonl",
                        extract=lambda path, content: [
                            {
                                "id": os.path.basename(os.path.dirname(path)),
                                "body": row["body"],
                                "author": row["author"],
                            }
                            for line in content.splitlines()
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
            """watch() yields insert events when a new file is created."""
            db = DirSQL(
                tmp_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE items (name TEXT)",
                        glob="**/*.json",
                        extract=lambda path, content: [json.loads(content)],
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

            # Wait for events with timeout
            try:
                await asyncio.wait_for(task, timeout=5.0)
            except asyncio.TimeoutError:
                pytest.fail("Timed out waiting for watch events")

            insert = next((e for e in events if e.action == "insert"), None)
            assert insert is not None, f"no insert event in {events!r}"
            assert insert.table == "items"
            assert insert.row["name"] == "apple"

        @pytest.mark.asyncio
        async def it_emits_delete_events_for_removed_files(tmp_dir):
            """watch() yields delete events when a file is removed."""
            # Create file before init
            with open(os.path.join(tmp_dir, "doomed.json"), "w") as f:
                json.dump({"name": "doomed"}, f)

            db = DirSQL(
                tmp_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE items (name TEXT)",
                        glob="**/*.json",
                        extract=lambda path, content: [json.loads(content)],
                    ),
                ],
            )
            await db.ready()

            # Confirm initial data
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

            # Delete the file
            os.remove(os.path.join(tmp_dir, "doomed.json"))

            try:
                await asyncio.wait_for(task, timeout=5.0)
            except asyncio.TimeoutError:
                pytest.fail("Timed out waiting for watch events")

            assert len(events) >= 1
            assert events[0].action == "delete"
            assert events[0].table == "items"
            assert events[0].row["name"] == "doomed"

            # DB should reflect deletion
            results = await db.query("SELECT * FROM items")
            assert len(results) == 0

        @pytest.mark.asyncio
        async def it_emits_update_events_for_modified_files(tmp_dir):
            """watch() yields update events when a file is modified."""
            with open(os.path.join(tmp_dir, "item.json"), "w") as f:
                json.dump({"name": "draft"}, f)

            db = DirSQL(
                tmp_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE items (name TEXT)",
                        glob="**/*.json",
                        extract=lambda path, content: [json.loads(content)],
                    ),
                ],
            )
            await db.ready()

            events = []

            async def collect_events():
                async for event in db.watch():
                    events.append(event)
                    if len(events) >= 1:
                        break

            task = asyncio.create_task(collect_events())
            await asyncio.sleep(0.3)

            # Modify the file
            with open(os.path.join(tmp_dir, "item.json"), "w") as f:
                json.dump({"name": "final"}, f)

            try:
                await asyncio.wait_for(task, timeout=5.0)
            except asyncio.TimeoutError:
                pytest.fail("Timed out waiting for watch events")

            assert len(events) >= 1
            # Could be update or delete+insert depending on diff logic
            actions = {e.action for e in events}
            assert "update" in actions or ("delete" in actions and "insert" in actions)

        @pytest.mark.asyncio
        async def it_emits_error_events_for_bad_extract(tmp_dir):
            """watch() yields error events when extract lambda fails."""
            db = DirSQL(
                tmp_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE items (name TEXT)",
                        glob="**/*.json",
                        extract=lambda path, content: [json.loads(content)],
                    ),
                ],
            )
            await db.ready()

            events = []

            async def collect_events():
                async for event in db.watch():
                    events.append(event)
                    if len(events) >= 1:
                        break

            task = asyncio.create_task(collect_events())
            await asyncio.sleep(0.3)

            # Create a file with invalid JSON
            with open(os.path.join(tmp_dir, "bad.json"), "w") as f:
                f.write("not json at all")

            try:
                await asyncio.wait_for(task, timeout=5.0)
            except asyncio.TimeoutError:
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
            """The database is kept in sync with file system changes."""
            db = DirSQL(
                tmp_dir,
                tables=[
                    Table(
                        ddl="CREATE TABLE items (name TEXT)",
                        glob="**/*.json",
                        extract=lambda path, content: [json.loads(content)],
                    ),
                ],
            )
            await db.ready()

            # Initially empty
            results = await db.query("SELECT * FROM items")
            assert len(results) == 0

            events = []

            async def collect_events():
                async for event in db.watch():
                    events.append(event)
                    if len(events) >= 1:
                        break

            task = asyncio.create_task(collect_events())
            await asyncio.sleep(0.3)

            # Add a file
            with open(os.path.join(tmp_dir, "new.json"), "w") as f:
                json.dump({"name": "added"}, f)

            try:
                await asyncio.wait_for(task, timeout=5.0)
            except asyncio.TimeoutError:
                pytest.fail("Timed out waiting for watch events")

            # DB should now have the row
            results = await db.query("SELECT * FROM items")
            assert len(results) == 1
            assert results[0]["name"] == "added"
