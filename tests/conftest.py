import json
import os
import tempfile

import pytest


@pytest.fixture
def tmp_dir():
    """Create a temporary directory for test files."""
    with tempfile.TemporaryDirectory() as d:
        yield d


@pytest.fixture
def jsonl_dir(tmp_dir):
    """Create a temp dir with JSONL files for testing."""
    # Create a simple JSONL file
    os.makedirs(os.path.join(tmp_dir, "comments", "abc"), exist_ok=True)
    os.makedirs(os.path.join(tmp_dir, "comments", "def"), exist_ok=True)

    with open(os.path.join(tmp_dir, "comments", "abc", "index.jsonl"), "w") as f:
        f.write(json.dumps({"body": "first comment", "author": "alice"}) + "\n")
        f.write(json.dumps({"body": "second comment", "author": "bob"}) + "\n")

    with open(os.path.join(tmp_dir, "comments", "def", "index.jsonl"), "w") as f:
        f.write(json.dumps({"body": "another comment", "author": "carol"}) + "\n")

    return tmp_dir


@pytest.fixture
def csv_dir(tmp_dir):
    """Create a temp dir with a CSV-like file for testing single-row extraction."""
    with open(os.path.join(tmp_dir, "metadata.json"), "w") as f:
        json.dump({"title": "My Project", "version": "1.0"}, f)

    return tmp_dir
