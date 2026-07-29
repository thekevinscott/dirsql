"""Shared fixtures for the integration and e2e tiers.

`stub_server` brings up a real, local, threaded OpenAI-compatible
`/v1/embeddings` endpoint. Its embedding is a deterministic keyword-count
vector, so nearest-neighbor search over the fixtures is reproducible without a
real model or network.

`make_pdf` builds a real PDF by hand rather than pulling in a writer library:
these tiers mock nothing, and a one-page catalog with a `BT ... Tj ET` content
stream is enough for pypdf to extract the text back out.
"""

import json
import threading
from http.server import BaseHTTPRequestHandler, HTTPServer

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


def _make_pdf(text):
    stream = f"BT /F1 24 Tf 72 700 Td ({text}) Tj ET".encode()
    objs = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
        b"/Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>",
        b"<< /Length "
        + str(len(stream)).encode()
        + b" >>\nstream\n"
        + stream
        + b"\nendstream",
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
    ]
    out = bytearray(b"%PDF-1.4\n")
    offsets = []
    for index, body in enumerate(objs, start=1):
        offsets.append(len(out))
        out += f"{index} 0 obj\n".encode() + body + b"\nendobj\n"
    xref = len(out)
    out += f"xref\n0 {len(objs) + 1}\n".encode()
    out += b"0000000000 65535 f \n"
    for offset in offsets:
        out += f"{offset:010d} 00000 n \n".encode()
    out += (
        f"trailer\n<< /Size {len(objs) + 1} /Root 1 0 R >>\n"
        f"startxref\n{xref}\n%%EOF\n"
    ).encode()
    return bytes(out)


@pytest.fixture
def make_pdf():
    return _make_pdf


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
