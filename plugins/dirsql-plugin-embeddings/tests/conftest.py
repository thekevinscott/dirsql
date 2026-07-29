"""Shared fixtures for the integration and e2e tiers.

`stub_server` brings up a real, local, threaded OpenAI-compatible
`/v1/embeddings` endpoint. Its embedding is a deterministic keyword-count
vector, so nearest-neighbor search over the fixtures is reproducible without a
real model or network.

Cache reads are switched off for both tiers. These tiers run the hooks as
subprocesses, which no in-process mock can reach, so a cached extraction from an
earlier run would otherwise decide this one's result. Writes are left alone --
they are harmless, and it is only the read that leaks state between runs.
"""

import json
import os
import threading
from http.server import BaseHTTPRequestHandler, HTTPServer

os.environ["DIRSQL_EMBEDDINGS_CACHE_READ"] = "0"

import pytest

# One dimension per keyword; the embedding of a text is the per-keyword count.
KEYWORDS = [
    "pasta",
    "cook",
    "garlic",
    "git",
    "code",
    "review",
    "tomato",
    "plant",
    "seed",
]


def keyword_vector(text):
    lowered = text.lower()
    return [float(lowered.count(keyword)) for keyword in KEYWORDS]


class _Handler(BaseHTTPRequestHandler):
    def do_POST(self):
        length = int(self.headers["Content-Length"])
        payload = json.loads(self.rfile.read(length))
        text = payload["input"][0]
        body = json.dumps(
            {"data": [{"index": 0, "embedding": keyword_vector(text)}]}
        ).encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *args):  # silence the default stderr access log
        pass


@pytest.fixture
def stub_server():
    server = HTTPServer(("127.0.0.1", 0), _Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    host, port = server.server_address
    try:
        yield f"http://{host}:{port}"
    finally:
        server.shutdown()
        thread.join()
