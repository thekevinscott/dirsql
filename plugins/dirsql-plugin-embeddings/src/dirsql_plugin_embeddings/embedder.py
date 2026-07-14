"""Embed text via an OpenAI-compatible ``/v1/embeddings`` endpoint.

RED stub: signatures only; behavior is unimplemented so the colocated unit
tests fail their assertions until the GREEN commit fills them in.
"""

from __future__ import annotations

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
    return 0, b""


def _require(env: Mapping[str, str], name: str) -> str:
    return ""


def embed(text: str, *, env: Mapping[str, str] | None = None, post=_urllib_post) -> list[float]:
    return []
