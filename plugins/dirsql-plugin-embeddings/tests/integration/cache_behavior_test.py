"""Integration: the on-disk vector cache, observed through real workers.

Each (value bytes, model identifier) pair owns exactly one cache entry under
``$XDG_CACHE_HOME/dirsql/embeddings/``, so entry counts across real worker
runs pin hit/miss behavior: repeats (same process or a fresh one) add nothing,
content changes and model changes each add one. The default-model requests are
driven through a wrapper process that points the default at the on-disk test
model (the sandboxed suite cannot download the real default); the worker
itself runs unmodified.
"""

import base64
import sys

DEFAULT_PATCH_ARGV_PREFIX = [
    sys.executable,
    "-c",
    "import sys\n"
    "from unittest.mock import patch\n"
    "from dirsql_plugin_embeddings import model\n"
    "from dirsql_plugin_embeddings.cli import main\n"
    "with patch.object(model, 'DEFAULT_MODEL_ID', sys.argv.pop(1)):\n"
    "    sys.exit(main())\n",
]


def entries(cache_home):
    embeddings = cache_home / "dirsql" / "embeddings"
    if not embeddings.is_dir():
        return 0
    return sum(1 for path in embeddings.iterdir() if path.is_file())


def describe_cache_location():
    def it_writes_under_xdg_cache_home_dirsql_embeddings(
        spawn_worker, cache_home, tiny_model
    ):
        worker = spawn_worker()
        worker.request("hello", tiny_model)
        assert entries(cache_home) == 1

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
        assert first == second == {"ok": [1.0, 0.0]}
        assert entries(cache_home) == 1

    def it_survives_the_process_a_fresh_worker_hits_the_same_entry(
        spawn_worker, cache_home, tiny_model
    ):
        first_worker = spawn_worker()
        first = first_worker.request("hello", tiny_model)
        first_worker.close()
        second_worker = spawn_worker()
        second = second_worker.request("hello", tiny_model)
        assert first == second == {"ok": [1.0, 0.0]}
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
        assert worker.request("hello", tiny_model) == {"ok": [1.0, 0.0]}
        assert worker.request("hello", other_model) == {"ok": [0.0, 2.0]}
        assert entries(cache_home) == 2


def describe_default_model():
    def it_shares_one_entry_between_default_and_explicit_same_id(
        spawn_worker, cache_home, tiny_model
    ):
        argv = DEFAULT_PATCH_ARGV_PREFIX + [tiny_model, "worker"]
        worker = spawn_worker(argv=argv)
        by_default = worker.send_line('{"call": ["hello"]}')
        explicit = worker.request("hello", tiny_model)
        assert by_default == explicit == {"ok": [1.0, 0.0]}
        assert entries(cache_home) == 1
