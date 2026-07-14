"""Embed text via an OpenAI-compatible ``/v1/embeddings`` endpoint.

Configuration comes from three environment variables (base URL, model, API
key). The HTTP call is behind an injected ``post`` seam so the unit tests drive
it without a real network; ``_urllib_post`` is the production seam.
"""

from __future__ import annotations

import json
import os
import urllib.error
import urllib.request
from collections.abc import Mapping

ENV_BASE_URL = "DIRSQL_EMBEDDINGS_BASE_URL"
ENV_MODEL = "DIRSQL_EMBEDDINGS_MODEL"
ENV_API_KEY = "DIRSQL_EMBEDDINGS_API_KEY"


class EmbeddingError(RuntimeError):
    """A configuration, transport, or response error while embedding."""


def _urllib_post(url: str, *, data: bytes, headers: Mapping[str, str]):
    request = urllib.request.Request(
        url, data=data, headers=dict(headers), method="POST"
    )
    try:
        with urllib.request.urlopen(request) as response:
            return response.status, response.read()
    except urllib.error.HTTPError as error:
        return error.code, error.read()


def _require(env: Mapping[str, str], name: str) -> str:
    value = env.get(name, "")
    if not value:
        raise EmbeddingError(f"missing required environment variable {name}")
    return value


def embed(
    text: str, *, env: Mapping[str, str] | None = None, post=_urllib_post
) -> list[float]:
    if env is None:
        env = os.environ
    base_url = _require(env, ENV_BASE_URL).rstrip("/")
    model = _require(env, ENV_MODEL)
    api_key = _require(env, ENV_API_KEY)

    data = json.dumps({"model": model, "input": [text]}).encode("utf-8")
    headers = {
        "Content-Type": "application/json",
        "Authorization": f"Bearer {api_key}",
    }
    status, body = post(f"{base_url}/v1/embeddings", data=data, headers=headers)
    if status != 200:
        raise EmbeddingError(f"embeddings endpoint returned status {status}: {body!r}")

    try:
        entry = json.loads(body)["data"][0]
    except (json.JSONDecodeError, KeyError, IndexError, TypeError) as exc:
        raise EmbeddingError(f"malformed embeddings response: {body!r}") from exc

    vector = entry.get("embedding")
    if not isinstance(vector, list) or not vector:
        raise EmbeddingError(f"embeddings response carried no vector: {body!r}")
    return [float(component) for component in vector]
