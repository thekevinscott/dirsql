import json
from hashlib import sha256
from unittest.mock import MagicMock, call, patch

from . import worker

HELLO_DIGEST = sha256(b"hello").hexdigest()


def make_worker():
    with patch.object(worker, "make_cache") as make_cache:
        built = worker.Worker()
    return built, make_cache.return_value


def describe_init():
    def it_wraps_compute_with_the_cache():
        built, cache = make_worker()
        cache.wrap.assert_called_once_with(built._compute)
        assert built._compute_cached is cache.wrap.return_value

    def it_starts_with_no_loaded_models_and_nothing_pending():
        built, _ = make_worker()
        assert built._models == {}
        assert built._pending is None
        assert built._computed is False


def describe_model_loading():
    def it_loads_a_model_once_and_memoizes_it():
        built, _ = make_worker()
        with patch.object(worker, "model") as model:
            first = built._model("m1")
            second = built._model("m1")
        model.load_model.assert_called_once_with("m1")
        assert first is second is model.load_model.return_value

    def it_loads_each_distinct_model_id():
        built, _ = make_worker()
        with patch.object(worker, "model") as model:
            built._model("m1")
            built._model("m2")
        assert model.load_model.call_args_list == [call("m1"), call("m2")]


def describe_compute():
    def it_records_that_it_ran_so_a_miss_is_distinguishable_from_a_hit():
        # _compute runs only on a cache miss, so it is the one place a miss is
        # observable -- cachetta's wrapper returns both cases identically.
        built, _ = make_worker()
        loaded = MagicMock()
        loaded.encode.return_value = [[1.0]]
        built._pending = ("hello", loaded)
        built._computed = False
        built._compute(HELLO_DIGEST, "m1")
        assert built._computed is True

    def it_encodes_the_pending_text_and_returns_plain_floats():
        built, _ = make_worker()
        loaded = MagicMock()
        loaded.encode.return_value = [[1, 2.5]]
        built._pending = ("hello", loaded)
        vector = built._compute(HELLO_DIGEST, "m1")
        loaded.encode.assert_called_once_with(["hello"], show_progress_bar=False)
        assert vector == [1.0, 2.5]
        assert all(isinstance(component, float) for component in vector)

    def it_never_shows_a_per_call_progress_bar_even_on_a_tty():
        # The protocol embeds exactly one value per round trip, so a per-call
        # bar can only ever say 1/1 -- one meaningless bar per embedded value.
        built, _ = make_worker()
        loaded = MagicMock()
        loaded.encode.return_value = [[0.0]]
        built._pending = ("hello", loaded)
        with patch("sys.stderr.isatty", return_value=True):
            built._compute(HELLO_DIGEST, "m1")
        loaded.encode.assert_called_once_with(["hello"], show_progress_bar=False)


def describe_embed():
    def it_keys_the_cache_on_the_sha256_of_the_text_and_the_identifier():
        built, _ = make_worker()
        built._compute_cached = MagicMock(return_value=[1.0])
        with patch.object(worker, "model") as model:
            model.model_identifier.return_value = "m1@9.9"
            result = built.embed("hello", "m1")
        model.load_model.assert_called_once_with("m1")
        model.model_identifier.assert_called_once_with(
            "m1", model.load_model.return_value
        )
        built._compute_cached.assert_called_once_with(HELLO_DIGEST, "m1@9.9")
        assert result == ([1.0], True)

    def it_reports_a_hit_when_compute_never_ran():
        built, _ = make_worker()
        built._compute_cached = MagicMock(return_value=[1.0])
        with patch.object(worker, "model"):
            _, cached = built.embed("hello", "m1")
        assert cached is True

    def it_reports_a_miss_when_compute_ran():
        built, _ = make_worker()

        def compute(digest, identifier):
            built._computed = True
            return [1.0]

        built._compute_cached = MagicMock(side_effect=compute)
        with patch.object(worker, "model"):
            _, cached = built.embed("hello", "m1")
        assert cached is False

    def it_clears_a_previous_calls_miss_before_consulting_the_cache():
        # Without the reset, one miss would make every later hit read as a
        # miss for the rest of the process.
        built, _ = make_worker()
        built._computed = True
        built._compute_cached = MagicMock(return_value=[1.0])
        with patch.object(worker, "model"):
            _, cached = built.embed("hello", "m1")
        assert cached is True

    def it_stages_the_text_and_model_for_compute():
        built, _ = make_worker()
        built._compute_cached = MagicMock(return_value=[1.0])
        with patch.object(worker, "model") as model:
            built.embed("hello", "m1")
        assert built._pending == ("hello", model.load_model.return_value)

    def it_hashes_the_utf8_bytes_of_the_text():
        built, _ = make_worker()
        built._compute_cached = MagicMock(return_value=[1.0])
        with patch.object(worker, "model") as model:
            model.model_identifier.return_value = "m1"
            built.embed("héllo", "m1")
        expected = sha256("héllo".encode("utf-8")).hexdigest()
        assert built._compute_cached.call_args == call(expected, "m1")


def describe_handle():
    def it_answers_ok_with_the_embedding():
        built, _ = make_worker()
        with patch.object(
            built, "embed", return_value=([1.0, 0.0], False)
        ) as embed:
            response = built.handle('{"call": ["hello", "m1"]}')
        embed.assert_called_once_with("hello", "m1")
        assert response == {"ok": [1.0, 0.0], "meta": {"cached": False}}

    def it_reports_a_cache_hit_in_the_response_metadata():
        built, _ = make_worker()
        with patch.object(built, "embed", return_value=([1.0], True)):
            response = built.handle('{"call": ["hello", "m1"]}')
        assert response == {"ok": [1.0], "meta": {"cached": True}}

    def it_uses_the_default_model_for_a_single_argument_call():
        built, _ = make_worker()
        with patch.object(built, "embed", return_value=([1.0], False)) as embed:
            with patch.object(worker, "model") as model:
                model.DEFAULT_MODEL_ID = "default/model"
                response = built.handle('{"call": ["hello"]}')
        embed.assert_called_once_with("hello", "default/model")
        assert response == {"ok": [1.0], "meta": {"cached": False}}

    def it_answers_err_on_invalid_json():
        built, _ = make_worker()
        response = built.handle("{not json")
        assert set(response) == {"err"}
        assert response["err"].startswith("malformed request: invalid JSON:")

    def it_answers_err_on_a_non_object_request():
        built, _ = make_worker()
        assert built.handle("[1, 2]") == {"err": worker.MALFORMED_SHAPE}

    def it_answers_err_when_call_is_missing():
        built, _ = make_worker()
        assert built.handle('{"other": []}') == {"err": worker.MALFORMED_SHAPE}

    def it_answers_err_when_call_is_not_a_list():
        built, _ = make_worker()
        assert built.handle('{"call": "hello"}') == {
            "err": worker.MALFORMED_ARITY
        }

    def it_answers_err_on_an_empty_call():
        built, _ = make_worker()
        assert built.handle('{"call": []}') == {"err": worker.MALFORMED_ARITY}

    def it_answers_err_on_three_arguments():
        built, _ = make_worker()
        assert built.handle('{"call": ["a", "b", "c"]}') == {
            "err": worker.MALFORMED_ARITY
        }

    def it_answers_err_with_the_protocol_message_for_a_bad_value():
        built, _ = make_worker()
        with patch.object(
            worker,
            "decode_value",
            side_effect=worker.ProtocolError("bad value"),
        ) as decode:
            response = built.handle('{"call": [7]}')
        decode.assert_called_once_with(7)
        assert response == {"err": "bad value"}

    def it_answers_ok_null_for_a_null_value():
        built, _ = make_worker()
        with patch.object(built, "embed") as embed:
            response = built.handle('{"call": [null]}')
        embed.assert_not_called()
        assert response == {"ok": None}

    def it_answers_err_for_a_non_text_model_id():
        built, _ = make_worker()
        with patch.object(built, "embed") as embed:
            response = built.handle('{"call": ["hello", 42]}')
        embed.assert_not_called()
        assert response == {"err": worker.MALFORMED_MODEL_ID}

    def it_answers_err_naming_the_model_when_embedding_fails():
        built, _ = make_worker()
        with patch.object(
            built, "embed", side_effect=RuntimeError("model exploded")
        ):
            response = built.handle('{"call": ["hello", "bad/model"]}')
        assert response == {
            "err": "embed('bad/model') failed: model exploded"
        }


def describe_serve():
    def it_writes_one_compact_json_line_per_request_and_flushes():
        built, _ = make_worker()
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
        built, _ = make_worker()
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
        built, _ = make_worker()
        stdout = MagicMock()
        with patch.object(built, "handle", return_value={"ok": None}) as handle:
            built.serve(["\n", "   \n", '{"call": ["a"]}\n'], stdout)
        handle.assert_called_once_with('{"call": ["a"]}\n')
        stdout.write.assert_called_once_with('{"ok":null}\n')
