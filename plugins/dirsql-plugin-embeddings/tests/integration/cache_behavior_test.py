"""Integration: the on-disk vector cache, observed through real workers.

The cache is one SQLite database per model under
``$XDG_CACHE_HOME/dirsql/embeddings/``, holding one row per (value bytes,
model identifier) pair, so row counts across real worker runs pin hit/miss
behavior: repeats (same process or a fresh one) add nothing, content changes
and model changes each add one. The default-model requests are driven through
a wrapper process that points the default at the on-disk test model (the
sandboxed suite cannot download the real default); the worker itself runs
unmodified.
"""

import base64
import json
import sqlite3
import sys
from contextlib import closing

DEFAULT_PATCH_ARGV_PREFIX = [
    sys.executable,
    "-c",
    "import sys\n"
    "from unittest.mock import patch\n"
    "from dirsql_plugin_embeddings.embedding import model\n"
    "from dirsql_plugin_embeddings.cli.main import main\n"
    "with patch.object(model, 'DEFAULT_MODEL_ID', sys.argv.pop(1)):\n"
    "    sys.exit(main())\n",
]


def databases(cache_home):
    embeddings = cache_home / "dirsql" / "embeddings"
    if not embeddings.is_dir():
        return []
    return sorted(embeddings.glob("*.db"))


def entries(cache_home):
    total = 0
    for path in databases(cache_home):
        with closing(sqlite3.connect(path)) as connection:
            (count,) = connection.execute("SELECT count(*) FROM vectors").fetchone()
        total += count
    return total


def describe_cache_location():
    def it_writes_under_xdg_cache_home_dirsql_embeddings(
        spawn_worker, cache_home, tiny_model
    ):
        worker = spawn_worker()
        worker.request("hello", tiny_model)
        assert entries(cache_home) == 1

    def it_keeps_one_database_per_model(
        spawn_worker, cache_home, tiny_model, other_model
    ):
        worker = spawn_worker()
        worker.request("hello", tiny_model)
        assert len(databases(cache_home)) == 1
        worker.request("hello", other_model)
        assert len(databases(cache_home)) == 2

    def it_never_writes_into_the_working_directory(
        spawn_worker, tiny_model, tmp_path
    ):
        queried_tree = tmp_path / "queried-tree"
        queried_tree.mkdir()
        worker = spawn_worker(cwd=str(queried_tree))
        worker.request("hello", tiny_model)
        worker.close()
        assert list(queried_tree.iterdir()) == []


def describe_cache_hits():
    def it_serves_a_repeat_of_the_same_content_and_model_from_one_entry(
        spawn_worker, cache_home, tiny_model
    ):
        worker = spawn_worker()
        first = worker.request("hello", tiny_model)
        second = worker.request("hello", tiny_model)
        assert first["ok"] == second["ok"] == [1.0, 0.0]
        assert entries(cache_home) == 1

    def it_survives_the_process_a_fresh_worker_hits_the_same_entry(
        spawn_worker, cache_home, tiny_model
    ):
        first_worker = spawn_worker()
        first = first_worker.request("hello", tiny_model)
        first_worker.close()
        second_worker = spawn_worker()
        second = second_worker.request("hello", tiny_model)
        assert first["ok"] == second["ok"] == [1.0, 0.0]
        assert entries(cache_home) == 1

    def it_treats_text_and_blob_of_the_same_bytes_as_one_value(
        spawn_worker, cache_home, tiny_model
    ):
        worker = spawn_worker()
        worker.request("hello", tiny_model)
        encoded = base64.b64encode(b"hello").decode("ascii")
        worker.request({"$bytes": encoded}, tiny_model)
        assert entries(cache_home) == 1


def describe_cache_misses():
    def it_misses_when_the_content_changes(
        spawn_worker, cache_home, tiny_model
    ):
        worker = spawn_worker()
        worker.request("hello", tiny_model)
        worker.request("world", tiny_model)
        assert entries(cache_home) == 2

    def it_misses_when_the_model_changes(
        spawn_worker, cache_home, tiny_model, other_model
    ):
        worker = spawn_worker()
        assert worker.request("hello", tiny_model)["ok"] == [1.0, 0.0]
        assert worker.request("hello", other_model)["ok"] == [0.0, 2.0]
        assert entries(cache_home) == 2


def describe_default_model():
    def it_shares_one_entry_between_default_and_explicit_same_id(
        spawn_worker, cache_home, tiny_model
    ):
        argv = DEFAULT_PATCH_ARGV_PREFIX + [tiny_model, "worker"]
        worker = spawn_worker(argv=argv)
        by_default = worker.send_line('{"call": ["hello"]}')
        explicit = worker.request("hello", tiny_model)
        assert by_default["ok"] == explicit["ok"] == [1.0, 0.0]
        assert entries(cache_home) == 1


def describe_batched_requests():
    def _batch(worker, *texts, model):
        line = json.dumps({"calls": [[text, model] for text in texts]})
        return worker.send_line(line)["results"]

    def it_adds_one_entry_per_distinct_value(spawn_worker, cache_home, tiny_model):
        worker = spawn_worker()
        _batch(worker, "hello", "world", "hello", model=tiny_model)
        assert entries(cache_home) == 2

    def it_serves_a_repeated_batch_from_the_cache(
        spawn_worker, cache_home, tiny_model
    ):
        worker = spawn_worker()
        cold = _batch(worker, "hello", "world", model=tiny_model)
        warm = _batch(worker, "hello", "world", model=tiny_model)
        assert [result["meta"] for result in cold] == [{"cached": False}] * 2
        assert [result["meta"] for result in warm] == [{"cached": True}] * 2
        assert [result["ok"] for result in warm] == [[1.0, 0.0], [0.0, 1.0]]
        assert entries(cache_home) == 2

    def it_computes_only_the_misses_of_a_mixed_batch(
        spawn_worker, cache_home, tiny_model
    ):
        worker = spawn_worker()
        worker.request("hello", tiny_model)
        hit, miss = _batch(worker, "hello", "world", model=tiny_model)
        assert hit["meta"] == {"cached": True}
        assert miss["meta"] == {"cached": False}
        assert entries(cache_home) == 2

    def it_survives_the_process(spawn_worker, cache_home, tiny_model):
        first = spawn_worker()
        _batch(first, "hello", "world", model=tiny_model)
        first.close()
        warm = _batch(spawn_worker(), "world", "hello", model=tiny_model)
        assert [result["ok"] for result in warm] == [[0.0, 1.0], [1.0, 0.0]]
        assert [result["meta"] for result in warm] == [{"cached": True}] * 2
        assert entries(cache_home) == 2


def describe_cache_reporting():
    """The worker tells dirsql which answers cost nothing.

    A hit is a row the store already held; a miss is a vector the model
    computed this call. These pin that the flag says which.
    """

    def it_flags_a_recomputed_value_as_not_cached(spawn_worker, tiny_model):
        worker = spawn_worker()
        response = worker.request("hello", tiny_model)
        assert response["meta"] == {"cached": False}

    def it_flags_a_repeat_as_served_from_cache(spawn_worker, tiny_model):
        worker = spawn_worker()
        worker.request("hello", tiny_model)
        response = worker.request("hello", tiny_model)
        assert response["meta"] == {"cached": True}

    def it_flags_a_hit_from_a_fresh_process(spawn_worker, tiny_model):
        first = spawn_worker()
        first.request("hello", tiny_model)
        first.close()
        response = spawn_worker().request("hello", tiny_model)
        assert response["meta"] == {"cached": True}

    def it_flags_each_distinct_value_as_computed(spawn_worker, tiny_model):
        worker = spawn_worker()
        first = worker.request("hello", tiny_model)
        second = worker.request("world", tiny_model)
        assert first["meta"] == second["meta"] == {"cached": False}

    def it_flags_a_new_model_for_a_seen_value_as_computed(
        spawn_worker, tiny_model, other_model
    ):
        worker = spawn_worker()
        worker.request("hello", tiny_model)
        response = worker.request("hello", other_model)
        assert response["meta"] == {"cached": False}

    def it_reports_no_cache_state_for_a_null_value(spawn_worker, tiny_model):
        worker = spawn_worker()
        assert worker.request(None, tiny_model) == {"ok": None}
