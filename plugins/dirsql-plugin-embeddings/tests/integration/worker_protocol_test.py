"""Integration: the worker's stdin/stdout protocol over real pipes.

Spawns the real ``worker`` subcommand as a subprocess and speaks the
newline-delimited JSON protocol: requests are ``{"call": [value, model_id?]}``
or, batched, ``{"calls": [[value, model_id?], ...]}``; responses
``{"ok": [floats...]}`` (plus an advisory ``"meta"``, covered in
cache_behavior_test) or ``{"err": "message"}``, one per call, wrapped in
``{"results": [...]}`` for a batch. The model is a real model2vec model on
disk (see conftest), passed through the ordinary model-override argument.
"""

import base64
import json


def describe_embed_requests():
    def it_embeds_sql_text_to_a_float_vector(spawn_worker, tiny_model):
        worker = spawn_worker()
        response = worker.request("hello", tiny_model)
        assert response["ok"] == [1.0, 0.0]

    def it_decodes_a_tagged_blob_as_utf8_text(spawn_worker, tiny_model):
        worker = spawn_worker()
        encoded = base64.b64encode("hello".encode("utf-8")).decode("ascii")
        response = worker.request({"$bytes": encoded}, tiny_model)
        assert response["ok"] == [1.0, 0.0]

    def it_passes_null_through_as_ok_null(spawn_worker, tiny_model):
        worker = spawn_worker()
        assert worker.request(None, tiny_model) == {"ok": None}

    def it_averages_token_vectors(spawn_worker, tiny_model):
        worker = spawn_worker()
        response = worker.request("hello world", tiny_model)
        assert response["ok"] == [0.5, 0.5]

    def it_answers_each_request_in_order_on_one_line_each(
        spawn_worker, tiny_model
    ):
        worker = spawn_worker()
        first = worker.request("hello", tiny_model)
        second = worker.request("world", tiny_model)
        third = worker.request("hello world", tiny_model)
        assert (first["ok"], second["ok"], third["ok"]) == (
            [1.0, 0.0],
            [0.0, 1.0],
            [0.5, 0.5],
        )

    def it_exits_cleanly_on_eof(spawn_worker, tiny_model):
        worker = spawn_worker()
        worker.request("hello", tiny_model)
        code, _ = worker.close()
        assert code == 0


def describe_malformed_requests():
    def it_answers_err_to_invalid_json_and_stays_alive(
        spawn_worker, tiny_model
    ):
        worker = spawn_worker()
        response = worker.send_line("{not json")
        assert "err" in response
        assert worker.request("hello", tiny_model)["ok"] == [1.0, 0.0]

    def it_rejects_a_request_without_call(spawn_worker, tiny_model):
        worker = spawn_worker()
        response = worker.send_line(json.dumps({"nope": []}))
        assert "err" in response
        assert worker.request("hello", tiny_model)["ok"] == [1.0, 0.0]

    def it_rejects_an_empty_call_list(spawn_worker, tiny_model):
        worker = spawn_worker()
        response = worker.send_line(json.dumps({"call": []}))
        assert "err" in response
        assert worker.request("hello", tiny_model)["ok"] == [1.0, 0.0]

    def it_rejects_more_than_two_arguments(spawn_worker, tiny_model):
        worker = spawn_worker()
        response = worker.request("hello", tiny_model, "extra")
        assert "err" in response
        assert worker.request("hello", tiny_model)["ok"] == [1.0, 0.0]

    def it_rejects_a_numeric_value(spawn_worker, tiny_model):
        worker = spawn_worker()
        response = worker.request(7, tiny_model)
        assert "err" in response
        assert worker.request("hello", tiny_model)["ok"] == [1.0, 0.0]

    def it_rejects_a_non_text_model_id(spawn_worker, tiny_model):
        worker = spawn_worker()
        response = worker.request("hello", 42)
        assert "err" in response
        assert worker.request("hello", tiny_model)["ok"] == [1.0, 0.0]

    def it_rejects_invalid_base64_bytes(spawn_worker, tiny_model):
        worker = spawn_worker()
        response = worker.request({"$bytes": "!!!not-base64!!!"}, tiny_model)
        assert "err" in response
        assert worker.request("hello", tiny_model)["ok"] == [1.0, 0.0]


def describe_unknown_models():
    def it_answers_err_naming_the_model_and_stays_alive(
        spawn_worker, tiny_model, tmp_path
    ):
        worker = spawn_worker()
        missing = str(tmp_path / "no-such-model")
        response = worker.request("hello", missing)
        assert "err" in response
        assert "no-such-model" in response["err"]
        assert worker.request("hello", tiny_model)["ok"] == [1.0, 0.0]


def describe_batched_requests():
    def _batch(worker, *calls):
        return worker.send_line(json.dumps({"calls": [list(c) for c in calls]}))

    def it_answers_results_in_request_order(spawn_worker, tiny_model):
        worker = spawn_worker()
        response = _batch(
            worker,
            ("hello", tiny_model),
            ("world", tiny_model),
            ("hello world", tiny_model),
        )
        assert [result["ok"] for result in response["results"]] == [
            [1.0, 0.0],
            [0.0, 1.0],
            [0.5, 0.5],
        ]

    def it_serves_two_models_in_one_request(
        spawn_worker, tiny_model, other_model
    ):
        worker = spawn_worker()
        response = _batch(
            worker,
            ("hello", tiny_model),
            ("hello", other_model),
            ("world", tiny_model),
        )
        assert [result["ok"] for result in response["results"]] == [
            [1.0, 0.0],
            [0.0, 2.0],
            [0.0, 1.0],
        ]

    def it_answers_each_malformed_call_in_place(spawn_worker, tiny_model):
        worker = spawn_worker()
        response = _batch(
            worker,
            ("hello", tiny_model),
            (7, tiny_model),
            (None, tiny_model),
            ("hello", 42),
        )
        ok, bad_value, null, bad_model = response["results"]
        assert ok["ok"] == [1.0, 0.0]
        assert "err" in bad_value
        assert null == {"ok": None}
        assert "err" in bad_model

    def it_fails_only_the_calls_of_an_unknown_model(
        spawn_worker, tiny_model, tmp_path
    ):
        worker = spawn_worker()
        missing = str(tmp_path / "no-such-model")
        response = _batch(worker, ("hello", missing), ("hello", tiny_model))
        failed, ok = response["results"]
        assert "no-such-model" in failed["err"]
        assert ok["ok"] == [1.0, 0.0]

    def it_answers_an_empty_batch_with_no_results(spawn_worker):
        worker = spawn_worker()
        assert _batch(worker) == {"results": []}

    def it_rejects_calls_that_is_not_a_list_and_stays_alive(
        spawn_worker, tiny_model
    ):
        worker = spawn_worker()
        response = worker.send_line(json.dumps({"calls": "hello"}))
        assert "err" in response
        assert worker.request("hello", tiny_model)["ok"] == [1.0, 0.0]

    def it_keeps_serving_single_calls_after_a_batch(spawn_worker, tiny_model):
        worker = spawn_worker()
        _batch(worker, ("hello", tiny_model), ("world", tiny_model))
        assert worker.request("hello world", tiny_model)["ok"] == [0.5, 0.5]
