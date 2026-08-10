"""Shared fixtures for the integration and e2e tiers.

``make_model`` builds a real model2vec model on disk: a three-token WordLevel
vocabulary with hand-picked vectors, so embeddings are deterministic and no
network or Hugging Face download is involved. The worker loads it through the
ordinary model-override argument (`{"call": [text, "<path>"]}`), exercising
the exact code path a hub model id takes.

``worker_process`` spawns the real worker subcommand as a subprocess over real
pipes, with ``XDG_CACHE_HOME`` pointed at a per-test temp dir so the vector
cache is isolated and its location is observable.
"""

import json
import os
import subprocess
import sys

import numpy as np
import pytest
from model2vec import StaticModel
from tokenizers import Tokenizer
from tokenizers.models import WordLevel
from tokenizers.pre_tokenizers import Whitespace

VOCAB = {"[UNK]": 0, "hello": 1, "world": 2}

WORKER_ARGV = [
    sys.executable,
    "-c",
    "import sys; from dirsql_plugin_embeddings.cli import main; sys.exit(main())",
    "worker",
]


def build_model(directory, rows):
    tokenizer = Tokenizer(WordLevel(VOCAB, unk_token="[UNK]"))
    tokenizer.pre_tokenizer = Whitespace()
    vectors = np.array(rows, dtype=np.float32)
    model = StaticModel(vectors, tokenizer, config={"normalize": False})
    model.save_pretrained(directory)
    return str(directory)


@pytest.fixture(scope="session")
def tiny_model(tmp_path_factory):
    # [UNK] -> [0, 0], hello -> [1, 0], world -> [0, 1].
    return build_model(
        tmp_path_factory.mktemp("model-a"),
        [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
    )


@pytest.fixture(scope="session")
def other_model(tmp_path_factory):
    # Same vocabulary, different vectors: hello -> [0, 2].
    return build_model(
        tmp_path_factory.mktemp("model-b"),
        [[0.0, 0.0], [0.0, 2.0], [2.0, 0.0]],
    )


class WorkerProcess:
    def __init__(self, argv, cache_home, cwd=None):
        self.process = subprocess.Popen(
            argv,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            cwd=cwd,
            env={**os.environ, "XDG_CACHE_HOME": str(cache_home)},
        )

    def send_line(self, line):
        self.process.stdin.write(line + "\n")
        self.process.stdin.flush()
        response = self.process.stdout.readline()
        assert response, (
            f"worker produced no response line; exited with"
            f" {self.process.poll()}, stderr: {self.process.stderr.read()!r}"
        )
        return json.loads(response)

    def request(self, *call):
        return self.send_line(json.dumps({"call": list(call)}))

    def close(self):
        self.process.stdin.close()
        code = self.process.wait(timeout=30)
        stderr = self.process.stderr.read()
        self.process.stdout.close()
        self.process.stderr.close()
        return code, stderr


@pytest.fixture
def cache_home(tmp_path):
    return tmp_path / "xdg-cache"


@pytest.fixture
def spawn_worker(cache_home):
    workers = []

    def spawn(argv=WORKER_ARGV, cwd=None):
        worker = WorkerProcess(argv, cache_home, cwd=cwd)
        workers.append(worker)
        return worker

    yield spawn
    for worker in workers:
        if worker.process.poll() is None:
            worker.process.kill()
        worker.process.wait()
