"""Integration: the worker's stdin/stdout protocol over real pipes.

Spawns the real ``worker`` subcommand as a subprocess and speaks the
newline-delimited JSON protocol from #801: requests are
``{"call": [value, model_id?]}``, responses ``{"ok": [floats...]}`` (plus an
advisory ``"meta"``, covered in cache_behavior_test) or ``{"err": "message"}``. The model is a real model2vec model on disk (see
conftest), passed through the ordinary model-override argument.
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
