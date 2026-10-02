from contextlib import contextmanager
from hashlib import sha256
from unittest.mock import MagicMock, call, patch

from . import worker

HELLO = sha256(b"hello").hexdigest()
WORLD = sha256(b"world").hexdigest()


@contextmanager
def loading(identifier="m1"):
    with patch.object(worker, "model") as model:
        with patch.object(
            worker, "model_identifier", return_value=identifier
        ) as identify:
            yield model, identify


def make_worker(found=None):
    with patch.object(worker, "VectorStore") as store_class:
        with patch.object(worker, "cache_dir") as cache_dir:
            built = worker.Worker()
    store = store_class.return_value
    store.lookup.return_value = {} if found is None else found
    return built, store, store_class, cache_dir


def encoding(loaded, vectors):
    loaded.encode.return_value.tolist.return_value = vectors


def describe_init():
    def it_opens_the_vector_store_at_the_cache_dir():
        built, store, store_class, cache_dir = make_worker()
        cache_dir.assert_called_once_with()
        store_class.assert_called_once_with(cache_dir.return_value)
        assert built._store is store

    def it_starts_with_no_loaded_models():
        built, *_ = make_worker()
        assert built._models == {}


def describe_model_loading():
    def it_loads_a_model_once_and_memoizes_it():
        built, *_ = make_worker()
        with patch.object(worker, "model") as model:
            first = built._model("m1")
            second = built._model("m1")
        model.load_model.assert_called_once_with("m1")
        assert first is second is model.load_model.return_value

    def it_loads_each_distinct_model_id():
        built, *_ = make_worker()
        with patch.object(worker, "model") as model:
            built._model("m1")
            built._model("m2")
        assert model.load_model.call_args_list == [call("m1"), call("m2")]


def describe_embed():
    def it_looks_the_whole_batch_up_under_the_model_identifier():
        built, store, *_ = make_worker()
        with loading("m1@9.9") as (model, identify):
            encoding(model.load_model.return_value, [[1.0], [2.0]])
            built.embed(["hello", "world"], "m1")
        model.load_model.assert_called_once_with("m1")
        identify.assert_called_once_with("m1", model.load_model.return_value)
        store.lookup.assert_called_once_with("m1@9.9", [HELLO, WORLD])

    def it_hashes_the_utf8_bytes_of_each_text():
        built, store, *_ = make_worker()
        with loading() as (model, _):
            encoding(model.load_model.return_value, [[1.0]])
            built.embed(["héllo"], "m1")
        expected = sha256("héllo".encode("utf-8")).hexdigest()
        store.lookup.assert_called_once_with("m1", [expected])

    def it_encodes_only_the_misses_in_one_call_and_stores_them():
        built, store, *_ = make_worker(found={HELLO: [1.0, 0.0]})
        with loading() as (model, _):
            loaded = model.load_model.return_value
            encoding(loaded, [[0.0, 1.0]])
            result = built.embed(["hello", "world"], "m1")
        loaded.encode.assert_called_once_with(["world"], show_progress_bar=False)
        store.insert.assert_called_once_with("m1", {WORLD: [0.0, 1.0]})
        assert result == [([1.0, 0.0], True), ([0.0, 1.0], False)]

    def it_reports_a_cold_batch_as_computed_in_request_order():
        built, store, *_ = make_worker()
        with loading() as (model, _):
            encoding(model.load_model.return_value, [[1.0], [2.0], [3.0]])
            result = built.embed(["a", "b", "c"], "m1")
        assert result == [([1.0], False), ([2.0], False), ([3.0], False)]

    def it_never_encodes_when_every_text_is_cached():
        built, store, *_ = make_worker(found={HELLO: [1.0], WORLD: [2.0]})
        with loading() as (model, _):
            loaded = model.load_model.return_value
            result = built.embed(["world", "hello"], "m1")
        loaded.encode.assert_not_called()
        store.insert.assert_not_called()
        assert result == [([2.0], True), ([1.0], True)]

    def it_encodes_a_repeated_miss_once():
        built, store, *_ = make_worker()
        with loading() as (model, _):
            loaded = model.load_model.return_value
            encoding(loaded, [[1.0]])
            result = built.embed(["hello", "hello"], "m1")
        loaded.encode.assert_called_once_with(["hello"], show_progress_bar=False)
        assert result == [([1.0], False), ([1.0], False)]

    def it_returns_the_vectors_as_plain_lists():
        built, *_ = make_worker()
        with loading() as (model, _):
            loaded = model.load_model.return_value
            encoding(loaded, [[1.0, 2.5]])
            ((vector, _),) = built.embed(["hello"], "m1")
        loaded.encode.return_value.tolist.assert_called_once_with()
        assert vector == [1.0, 2.5]


def describe_handle():
    def it_answers_ok_with_the_embedding():
        built, *_ = make_worker()
        with patch.object(
            built, "embed", return_value=[([1.0, 0.0], False)]
        ) as embed:
            response = built.handle('{"call": ["hello", "m1"]}')
        embed.assert_called_once_with(["hello"], "m1")
        assert response == {"ok": [1.0, 0.0], "meta": {"cached": False}}

    def it_reports_a_cache_hit_in_the_response_metadata():
        built, *_ = make_worker()
        with patch.object(built, "embed", return_value=[([1.0], True)]):
            response = built.handle('{"call": ["hello", "m1"]}')
        assert response == {"ok": [1.0], "meta": {"cached": True}}

    def it_uses_the_default_model_for_a_single_argument_call():
        built, *_ = make_worker()
        with patch.object(built, "embed", return_value=[([1.0], False)]) as embed:
            with patch.object(worker, "model") as model:
                model.DEFAULT_MODEL_ID = "default/model"
                response = built.handle('{"call": ["hello"]}')
        embed.assert_called_once_with(["hello"], "default/model")
        assert response == {"ok": [1.0], "meta": {"cached": False}}

    def it_answers_err_on_invalid_json():
        built, *_ = make_worker()
        response = built.handle("{not json")
        assert set(response) == {"err"}
        assert response["err"].startswith("malformed request: invalid JSON:")

    def it_answers_err_on_a_non_object_request():
        built, *_ = make_worker()
        assert built.handle("[1, 2]") == {"err": worker.MALFORMED_SHAPE}

    def it_answers_err_when_call_is_missing():
        built, *_ = make_worker()
        assert built.handle('{"other": []}') == {"err": worker.MALFORMED_SHAPE}

    def it_answers_err_when_call_is_not_a_list():
        built, *_ = make_worker()
        assert built.handle('{"call": "hello"}') == {
            "err": worker.MALFORMED_ARITY
        }

    def it_answers_err_on_an_empty_call():
        built, *_ = make_worker()
        assert built.handle('{"call": []}') == {"err": worker.MALFORMED_ARITY}

    def it_answers_err_on_three_arguments():
        built, *_ = make_worker()
        assert built.handle('{"call": ["a", "b", "c"]}') == {
            "err": worker.MALFORMED_ARITY
        }

    def it_answers_err_with_the_protocol_message_for_a_bad_value():
        built, *_ = make_worker()
        with patch.object(
            worker,
            "decode_value",
            side_effect=worker.ProtocolError("bad value"),
        ) as decode:
            response = built.handle('{"call": [7]}')
        decode.assert_called_once_with(7)
        assert response == {"err": "bad value"}

    def it_answers_ok_null_for_a_null_value():
        built, *_ = make_worker()
        with patch.object(built, "embed") as embed:
            response = built.handle('{"call": [null]}')
        embed.assert_not_called()
        assert response == {"ok": None}

    def it_answers_err_for_a_non_text_model_id():
        built, *_ = make_worker()
        with patch.object(built, "embed") as embed:
            response = built.handle('{"call": ["hello", 42]}')
        embed.assert_not_called()
        assert response == {"err": worker.MALFORMED_MODEL_ID}

    def it_answers_err_naming_the_model_when_embedding_fails():
        built, *_ = make_worker()
        with patch.object(
            built, "embed", side_effect=RuntimeError("model exploded")
        ):
            response = built.handle('{"call": ["hello", "bad/model"]}')
        assert response == {
            "err": "embed('bad/model') failed: model exploded"
        }


def describe_handle_batched():
    def it_answers_results_in_request_order():
        built, *_ = make_worker()
        with patch.object(
            built, "embed", return_value=[([1.0], False), ([2.0], True)]
        ) as embed:
            response = built.handle(
                '{"calls": [["hello", "m1"], ["world", "m1"]]}'
            )
        embed.assert_called_once_with(["hello", "world"], "m1")
        assert response == {
            "results": [
                {"ok": [1.0], "meta": {"cached": False}},
                {"ok": [2.0], "meta": {"cached": True}},
            ]
        }

    def it_embeds_once_per_model_and_restores_the_request_order():
        built, *_ = make_worker()
        by_model = {
            "m1": [([1.0], False), ([3.0], False)],
            "m2": [([2.0], False)],
        }
        with patch.object(
            built, "embed", side_effect=lambda texts, model_id: by_model[model_id]
        ) as embed:
            response = built.handle(
                '{"calls": [["a", "m1"], ["b", "m2"], ["c", "m1"]]}'
            )
        assert embed.call_args_list == [
            call(["a", "c"], "m1"),
            call(["b"], "m2"),
        ]
        assert [result["ok"] for result in response["results"]] == [
            [1.0],
            [2.0],
            [3.0],
        ]

    def it_answers_each_malformed_call_in_place_and_embeds_the_rest():
        built, *_ = make_worker()
        with patch.object(
            built, "embed", return_value=[([1.0], False)]
        ) as embed:
            response = built.handle(
                '{"calls": [[null, "m1"], ["a", "b", "c"], ["hello", 42],'
                ' ["hello", "m1"]]}'
            )
        embed.assert_called_once_with(["hello"], "m1")
        assert response == {
            "results": [
                {"ok": None},
                {"err": worker.MALFORMED_ARITY},
                {"err": worker.MALFORMED_MODEL_ID},
                {"ok": [1.0], "meta": {"cached": False}},
            ]
        }

    def it_fails_every_call_of_a_model_that_fails_and_keeps_serving_the_others():
        built, *_ = make_worker()

        def embed(texts, model_id):
            if model_id == "bad":
                raise RuntimeError("model exploded")
            return [([1.0], False)] * len(texts)

        with patch.object(built, "embed", side_effect=embed):
            response = built.handle(
                '{"calls": [["a", "bad"], ["b", "good"], ["c", "bad"]]}'
            )
        failure = {"err": "embed('bad') failed: model exploded"}
        assert response == {
            "results": [failure, {"ok": [1.0], "meta": {"cached": False}}, failure]
        }

    def it_uses_the_default_model_for_single_argument_calls():
        built, *_ = make_worker()
        with patch.object(built, "embed", return_value=[([1.0], False)]) as embed:
            with patch.object(worker, "model") as model:
                model.DEFAULT_MODEL_ID = "default/model"
                built.handle('{"calls": [["hello"]]}')
        embed.assert_called_once_with(["hello"], "default/model")

    def it_answers_no_results_for_an_empty_batch():
        built, *_ = make_worker()
        with patch.object(built, "embed") as embed:
            response = built.handle('{"calls": []}')
        embed.assert_not_called()
        assert response == {"results": []}

    def it_answers_err_when_calls_is_not_a_list():
        built, *_ = make_worker()
        with patch.object(built, "embed") as embed:
            response = built.handle('{"calls": "hello"}')
        embed.assert_not_called()
        assert response == {"err": worker.MALFORMED_CALLS}


def describe_serve():
    def it_writes_one_compact_json_line_per_request_and_flushes():
        built, *_ = make_worker()
        stdout = MagicMock()
        with patch.object(
            built, "handle", side_effect=[{"ok": [1.0]}, {"err": "x"}]
        ) as handle:
            built.serve(['{"call": ["a"]}\n', '{"call": ["b"]}\n'], stdout)
        assert handle.call_args_list == [
            call('{"call": ["a"]}\n'),
            call('{"call": ["b"]}\n'),
        ]
        assert stdout.write.call_args_list == [
            call('{"ok":[1.0]}\n'),
            call('{"err":"x"}\n'),
        ]
        assert stdout.flush.call_count == 2

    def it_flushes_after_each_line_not_only_at_the_end():
        built, *_ = make_worker()
        stdout = MagicMock()
        events = []
        stdout.write.side_effect = lambda line: events.append(("write", line))
        stdout.flush.side_effect = lambda: events.append(("flush",))
        with patch.object(built, "handle", return_value={"ok": None}):
            built.serve(["one\n", "two\n"], stdout)
        assert events == [
            ("write", '{"ok":null}\n'),
            ("flush",),
            ("write", '{"ok":null}\n'),
            ("flush",),
        ]

    def it_skips_blank_lines_and_keeps_serving_later_requests():
        built, *_ = make_worker()
        stdout = MagicMock()
        with patch.object(built, "handle", return_value={"ok": None}) as handle:
            built.serve(["\n", "   \n", '{"call": ["a"]}\n'], stdout)
        handle.assert_called_once_with('{"call": ["a"]}\n')
        stdout.write.assert_called_once_with('{"ok":null}\n')
