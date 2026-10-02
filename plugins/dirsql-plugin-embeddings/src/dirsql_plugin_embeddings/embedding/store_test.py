from hashlib import sha256
from unittest.mock import MagicMock, call, patch

from . import store


def make_store():
    directory = MagicMock()
    return store.VectorStore(directory), directory


def describe_databases():
    def it_keeps_one_database_per_model_named_by_the_identifier_digest():
        built, directory = make_store()
        with patch.object(store.sqlite3, "connect") as connect:
            built.lookup("m1", ["d"])
            built.lookup("m2", ["d"])
        assert directory.__truediv__.call_args_list == [
            call(sha256(b"m1").hexdigest() + ".db"),
            call(sha256(b"m2").hexdigest() + ".db"),
        ]
        assert connect.call_args_list == [call(directory / "x")] * 2

    def it_creates_the_cache_directory_before_opening():
        built, directory = make_store()
        with patch.object(store.sqlite3, "connect"):
            built.lookup("m1", [])
        directory.mkdir.assert_called_once_with(parents=True, exist_ok=True)

    def it_creates_the_table_on_open():
        built, _ = make_store()
        with patch.object(store.sqlite3, "connect") as connect:
            built.lookup("m1", [])
        assert connect.return_value.execute.call_args_list[0] == call(store.SCHEMA)

    def it_opens_each_model_once():
        built, _ = make_store()
        with patch.object(store.sqlite3, "connect") as connect:
            built.lookup("m1", ["a"])
            built.insert("m1", {"a": [1.0]})
            built.lookup("m1", ["a"])
        assert connect.call_count == 1


def describe_lookup():
    def it_finds_nothing_on_a_cold_lookup():
        built, _ = make_store()
        with patch.object(store.sqlite3, "connect") as connect:
            connect.return_value.execute.return_value = []
            assert built.lookup("m1", ["a", "b"]) == {}

    def it_unpacks_every_stored_row_keyed_by_digest():
        built, _ = make_store()
        with patch.object(store.sqlite3, "connect") as connect:
            connect.return_value.execute.return_value = [
                ("a", store.pack([1.0, -0.5])),
                ("c", store.pack([3.0])),
            ]
            found = built.lookup("m1", ["a", "b", "c"])
        assert found == {"a": [1.0, -0.5], "c": [3.0]}

    def it_asks_for_the_whole_batch_in_one_query():
        built, _ = make_store()
        with patch.object(store.sqlite3, "connect") as connect:
            connect.return_value.execute.return_value = []
            built.lookup("m1", ["a", "b", "c"])
        selects = [
            args
            for args, _ in connect.return_value.execute.call_args_list
            if args[0] is store.LOOKUP
        ]
        assert selects == [(store.LOOKUP, ('["a", "b", "c"]',))]

    def it_accepts_any_iterable_of_digests():
        built, _ = make_store()
        with patch.object(store.sqlite3, "connect") as connect:
            connect.return_value.execute.return_value = []
            built.lookup("m1", iter(["a", "b"]))
        connect.return_value.execute.assert_called_with(
            store.LOOKUP, ('["a", "b"]',)
        )


def describe_insert():
    def it_writes_the_packed_batch_in_one_transaction():
        built, _ = make_store()
        with patch.object(store.sqlite3, "connect") as connect:
            connection = connect.return_value
            built.insert("m1", {"a": [1.0], "b": [2.0, 3.0]})
        connection.__enter__.assert_called_once()
        connection.__exit__.assert_called_once()
        connection.executemany.assert_called_once_with(
            store.INSERT,
            [("a", store.pack([1.0])), ("b", store.pack([2.0, 3.0]))],
        )

    def it_writes_inside_the_transaction_not_before_it():
        built, _ = make_store()
        events = []
        with patch.object(store.sqlite3, "connect") as connect:
            connection = connect.return_value
            connection.__enter__.side_effect = lambda: events.append("enter")
            connection.executemany.side_effect = lambda *_: events.append("write")
            built.insert("m1", {"a": [1.0]})
        assert events == ["enter", "write"]

    def it_accepts_an_empty_batch():
        built, _ = make_store()
        with patch.object(store.sqlite3, "connect") as connect:
            built.insert("m1", {})
        connect.return_value.executemany.assert_called_once_with(store.INSERT, [])


def describe_pack():
    def it_encodes_doubles_and_unpack_restores_them_exactly():
        vector = [0.1, -2.5e-8, 1e300, 0.0, 3.0]
        assert store.unpack(store.pack(vector)) == vector

    def it_encodes_eight_bytes_per_component():
        assert len(store.pack([1.0, 2.0, 3.0])) == 24

    def it_unpacks_an_empty_blob_to_an_empty_vector():
        assert store.unpack(b"") == []
