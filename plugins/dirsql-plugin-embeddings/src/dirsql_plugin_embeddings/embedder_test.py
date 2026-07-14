"""Colocated unit tests for the embedder (isolation).

The HTTP call is driven through the injected `post` seam and configuration
through an injected `env` mapping, so no real network or process environment is
touched. `_urllib_post` is covered with `urllib` mocked.
"""

import io
import json
import urllib.error
from unittest import mock

import pytest

from . import embedder as module
from .embedder import (
    ENV_API_KEY,
    ENV_BASE_URL,
    ENV_MODEL,
    EmbeddingError,
    _require,
    _urllib_post,
    embed,
)

_ENV = {ENV_BASE_URL: "http://host", ENV_MODEL: "my-model", ENV_API_KEY: "secret"}


def _post(vector, *, status=200, raw=None):
    """A `post` seam recording its call and returning a canned response."""

    def post(url, *, data, headers):
        post.calls.append({"url": url, "data": data, "headers": headers})
        body = (
            raw
            if raw is not None
            else json.dumps({"data": [{"embedding": vector}]}).encode()
        )
        return status, body

    post.calls = []
    return post


def describe_require():
    def it_returns_a_present_value():
        assert _require({"K": "v"}, "K") == "v"

    def it_raises_naming_the_missing_variable():
        with pytest.raises(
            EmbeddingError, match="missing required environment variable MISSING"
        ):
            _require({}, "MISSING")

    def it_treats_empty_string_as_missing():
        with pytest.raises(EmbeddingError, match="ONE"):
            _require({"ONE": ""}, "ONE")


def describe_embed():
    def it_posts_the_expected_url_body_and_headers():
        post = _post([1.0, 2.0])
        embed("hello world", env=_ENV, post=post)
        (call,) = post.calls
        assert call["url"] == "http://host/v1/embeddings"
        assert json.loads(call["data"]) == {
            "model": "my-model",
            "input": ["hello world"],
        }
        assert call["headers"] == {
            "Content-Type": "application/json",
            "Authorization": "Bearer secret",
        }

    def it_strips_a_trailing_slash_from_the_base_url():
        post = _post([1.0])
        embed("x", env={**_ENV, ENV_BASE_URL: "http://host/"}, post=post)
        assert post.calls[0]["url"] == "http://host/v1/embeddings"

    def it_returns_floats_from_an_integer_vector():
        vector = embed("x", env=_ENV, post=_post([1, 2, 3]))
        assert vector == [1.0, 2.0, 3.0]
        assert all(isinstance(component, float) for component in vector)

    def it_reads_configuration_from_os_environ_by_default():
        post = _post([9.0])
        with mock.patch.dict(module.os.environ, _ENV, clear=True):
            assert embed("x", post=post) == [9.0]

    def it_raises_when_base_url_is_missing():
        with pytest.raises(EmbeddingError, match=ENV_BASE_URL):
            embed("x", env={ENV_MODEL: "m", ENV_API_KEY: "k"}, post=_post([1.0]))

    def it_raises_when_model_is_missing():
        with pytest.raises(EmbeddingError, match=ENV_MODEL):
            embed(
                "x", env={ENV_BASE_URL: "http://h", ENV_API_KEY: "k"}, post=_post([1.0])
            )

    def it_raises_when_api_key_is_missing():
        with pytest.raises(EmbeddingError, match=ENV_API_KEY):
            embed(
                "x", env={ENV_BASE_URL: "http://h", ENV_MODEL: "m"}, post=_post([1.0])
            )

    def it_raises_on_a_non_200_status():
        with pytest.raises(EmbeddingError, match="status 500"):
            embed("x", env=_ENV, post=_post([1.0], status=500, raw=b"boom"))

    def it_raises_on_non_json_output():
        with pytest.raises(EmbeddingError, match="malformed embeddings response"):
            embed("x", env=_ENV, post=_post(None, raw=b"not json"))

    def it_raises_when_data_key_is_absent():
        with pytest.raises(EmbeddingError, match="malformed embeddings response"):
            embed("x", env=_ENV, post=_post(None, raw=b'{"nope": 1}'))

    def it_raises_when_data_list_is_empty():
        with pytest.raises(EmbeddingError, match="malformed embeddings response"):
            embed("x", env=_ENV, post=_post(None, raw=b'{"data": []}'))

    def it_raises_when_embedding_is_not_a_list():
        with pytest.raises(EmbeddingError, match="no vector"):
            embed("x", env=_ENV, post=_post("not-a-list"))

    def it_raises_when_embedding_is_empty():
        with pytest.raises(EmbeddingError, match="no vector"):
            embed("x", env=_ENV, post=_post([]))


def describe_urllib_post():
    def it_returns_status_and_body_on_success():
        response = mock.MagicMock()
        response.status = 200
        response.read.return_value = b"payload"
        with mock.patch.object(module.urllib.request, "urlopen") as urlopen:
            urlopen.return_value.__enter__.return_value = response
            status, body = _urllib_post(
                "http://host/v1/embeddings", data=b"{}", headers={"H": "v"}
            )
        assert (status, body) == (200, b"payload")
        request = urlopen.call_args.args[0]
        assert request.get_method() == "POST"
        assert request.data == b"{}"

    def it_returns_the_error_code_and_body_on_http_error():
        error = urllib.error.HTTPError(
            "http://host", 429, "Too Many", {}, io.BytesIO(b"slow down")
        )
        with mock.patch.object(module.urllib.request, "urlopen", side_effect=error):
            status, body = _urllib_post("http://host", data=b"{}", headers={})
        assert (status, body) == (429, b"slow down")
