import sqlite3
from hashlib import sha256
from unittest.mock import MagicMock, call, patch

from . import store


def in_memory(_path):
    return sqlite3.connect(":memory:")


def make_store():
    directory = MagicMock()
    return store.VectorStore(directory), directory


def describe_databases():
    def it_keeps_one_database_per_model_named_by_the_identifier_digest():
        built, directory = make_store()
        with patch.object(store.sqlite3, "connect", side_effect=in_memory) as connect:
            built.lookup("m1", ["d"])
            built.lookup("m2", ["d"])
        assert directory.__truediv__.call_args_list == [
            call(sha256(b"m1").hexdigest() + ".db"),
            call(sha256(b"m2").hexdigest() + ".db"),
        ]
        assert connect.call_args_list == [call(directory / "x")] * 2

    def it_creates_the_cache_directory_before_opening():
        built, directory = make_store()
        with patch.object(store.sqlite3, "connect", side_effect=in_memory):
            built.lookup("m1", [])
        directory.mkdir.assert_called_once_with(parents=True, exist_ok=True)

    def it_opens_each_model_once():
        built, _ = make_store()
        with patch.object(store.sqlite3, "connect", side_effect=in_memory) as connect:
            built.lookup("m1", ["a"])
            built.insert("m1", {"a": [1.0]})
            built.lookup("m1", ["a"])
        assert connect.call_count == 1


def describe_lookup():
    def it_finds_nothing_on_a_cold_lookup():
        built, _ = make_store()
        with patch.object(store.sqlite3, "connect", side_effect=in_memory):
            assert built.lookup("m1", ["a", "b"]) == {}

    def it_serves_an_inserted_vector_on_a_warm_lookup():
        built, _ = make_store()
        with patch.object(store.sqlite3, "connect", side_effect=in_memory):
            built.insert("m1", {"a": [1.0, -0.5]})
            assert built.lookup("m1", ["a"]) == {"a": [1.0, -0.5]}

    def it_returns_only_the_stored_digests_of_a_mixed_batch():
        built, _ = make_store()
        with patch.object(store.sqlite3, "connect", side_effect=in_memory):
            built.insert("m1", {"a": [1.0], "c": [3.0]})
            found = built.lookup("m1", ["a", "b", "c", "d"])
        assert found == {"a": [1.0], "c": [3.0]}

    def it_keeps_models_apart():
        built, _ = make_store()
        with patch.object(store.sqlite3, "connect", side_effect=in_memory):
            built.insert("m1", {"a": [1.0]})
            assert built.lookup("m2", ["a"]) == {}

    def it_round_trips_vectors_exactly():
        built, _ = make_store()
        vector = [0.1, -2.5e-8, 1e300, 0.0, 3.0]
        with patch.object(store.sqlite3, "connect", side_effect=in_memory):
            built.insert("m1", {"a": vector})
            assert built.lookup("m1", ["a"]) == {"a": vector}

    def it_runs_one_query_for_the_whole_batch():
        built, _ = make_store()
        connection = MagicMock()
        connection.execute.return_value = []
        with patch.object(store.sqlite3, "connect", return_value=connection):
            built.lookup("m1", ["a", "b", "c"])
        selects = [
            args for args, _ in connection.execute.call_args_list
            if args[0].startswith("SELECT")
        ]
        assert selects == [(store.LOOKUP, ('["a", "b", "c"]',))]


def describe_insert():
    def it_replaces_an_existing_digest():
        built, _ = make_store()
        with patch.object(store.sqlite3, "connect", side_effect=in_memory):
            built.insert("m1", {"a": [1.0]})
            built.insert("m1", {"a": [2.0]})
            assert built.lookup("m1", ["a"]) == {"a": [2.0]}

    def it_writes_the_batch_in_one_transaction():
        built, _ = make_store()
        connection = MagicMock()
        with patch.object(store.sqlite3, "connect", return_value=connection):
            built.insert("m1", {"a": [1.0], "b": [2.0]})
        connection.__enter__.assert_called_once()
        connection.executemany.assert_called_once_with(
            store.INSERT,
            [("a", store.pack([1.0])), ("b", store.pack([2.0]))],
        )

    def it_accepts_an_empty_batch():
        built, _ = make_store()
        with patch.object(store.sqlite3, "connect", side_effect=in_memory):
            built.insert("m1", {})
            assert built.lookup("m1", ["a"]) == {}


def describe_pack():
    def it_encodes_doubles_and_unpack_restores_them():
        vector = [1.5, -2.0, 0.25]
        assert store.unpack(store.pack(vector)) == vector

    def it_encodes_eight_bytes_per_component():
        assert len(store.pack([1.0, 2.0, 3.0])) == 24
