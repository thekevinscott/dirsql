"""Colocated unit test for `read_pdf` (isolation).

`PdfReader` is mocked: this unit owns page traversal, joining, and the
disk-persisted cache, not pypdf's parsing. Real extraction from a real PDF is
covered by the integration and e2e tiers once `read_pdf` is wired to the
console script.

Every test redirects the cache at `tmp_path` through the
`DIRSQL_EMBEDDINGS_CACHE_DIR` override, so no test ever reads or writes a real
user cache.
"""

import hashlib
import json
import os
from datetime import timedelta
from unittest import mock

import pytest

from . import read_pdf as module
from .read_pdf import read_pdf

ENV_CACHE_DIR = "DIRSQL_EMBEDDINGS_CACHE_DIR"

# `os.utime` writes these verbatim, so the expected cache key below is exact
# rather than whatever the clock read when the fixture was written.
MTIME = 1_000_000_000
LATER_MTIME = 2_000_000_000


def _reader(*pages):
    return mock.Mock(pages=[mock.Mock(**{"extract_text.return_value": p}) for p in pages])


def _cache_at(tmp_path):
    return mock.patch.dict(os.environ, {ENV_CACHE_DIR: str(tmp_path / "cache")})


def _pdf(tmp_path, name="paper.pdf", mtime=MTIME):
    target = tmp_path / name
    target.write_bytes(b"%PDF-1.4 never parsed here, PdfReader is mocked")
    os.utime(target, (mtime, mtime))
    return target


def describe_read_pdf():
    def it_extracts_text_from_a_single_page(tmp_path):
        with (
            _cache_at(tmp_path),
            mock.patch.object(module, "PdfReader", return_value=_reader("hello")),
        ):
            assert read_pdf(str(_pdf(tmp_path))) == "hello"

    def it_joins_every_page_in_order(tmp_path):
        with (
            _cache_at(tmp_path),
            mock.patch.object(
                module, "PdfReader", return_value=_reader("one", "two", "three")
            ),
        ):
            assert read_pdf(str(_pdf(tmp_path))) == "one\ntwo\nthree"

    def it_opens_the_path_it_is_given(tmp_path):
        pdf = _pdf(tmp_path)
        with (
            _cache_at(tmp_path),
            mock.patch.object(module, "PdfReader", return_value=_reader("x")) as reader,
        ):
            read_pdf(str(pdf))
        reader.assert_called_once_with(str(pdf))

    def it_returns_empty_text_for_a_scanned_page_rather_than_raising(tmp_path):
        # pypdf yields '' for an image-only page; that is a file with no text,
        # not a failure, and matches how an empty .md already behaves.
        with (
            _cache_at(tmp_path),
            mock.patch.object(module, "PdfReader", return_value=_reader("")),
        ):
            assert read_pdf(str(_pdf(tmp_path, name="scan.pdf"))) == ""

    def it_propagates_an_extraction_failure(tmp_path):
        boom = ValueError("corrupt xref")
        with (
            _cache_at(tmp_path),
            mock.patch.object(module, "PdfReader", side_effect=boom),
            pytest.raises(ValueError) as caught,
        ):
            read_pdf(str(_pdf(tmp_path, name="broken.pdf")))
        assert caught.value is boom


def describe_caching():
    def it_does_not_re_extract_an_unchanged_pdf(tmp_path):
        pdf = _pdf(tmp_path)
        with (
            _cache_at(tmp_path),
            mock.patch.object(module, "PdfReader", return_value=_reader("once")) as reader,
        ):
            assert read_pdf(str(pdf)) == "once"
            assert read_pdf(str(pdf)) == "once"
        assert reader.call_count == 1

    def it_re_extracts_a_pdf_whose_mtime_moved(tmp_path):
        pdf = _pdf(tmp_path)
        with (
            _cache_at(tmp_path),
            mock.patch.object(module, "PdfReader", return_value=_reader("again")) as reader,
        ):
            read_pdf(str(pdf))
            os.utime(pdf, (LATER_MTIME, LATER_MTIME))
            read_pdf(str(pdf))
        assert reader.call_count == 2

    def it_keys_the_cache_file_on_the_path_and_the_mtime(tmp_path):
        # Recomputed here rather than imported from `cachetta.hash`: the point
        # is to pin the *observed* keying (a 16-char sha256 prefix over the
        # canonical JSON encoding of the call's arguments), so an upstream
        # change to it fails loudly instead of silently agreeing with itself.
        pdf = _pdf(tmp_path)
        with (
            _cache_at(tmp_path),
            mock.patch.object(module, "PdfReader", return_value=_reader("keyed")),
        ):
            read_pdf(str(pdf))
        key = json.dumps(
            {"args": [str(pdf), float(MTIME)], "kwargs": {}}, sort_keys=True, default=str
        )
        digest = hashlib.sha256(key.encode()).hexdigest()[:16]
        assert (tmp_path / "cache" / "pdf-text" / digest).is_file()

    def it_writes_one_cache_entry_per_distinct_pdf(tmp_path):
        with (
            _cache_at(tmp_path),
            mock.patch.object(module, "PdfReader", return_value=_reader("body")),
        ):
            read_pdf(str(_pdf(tmp_path, name="one.pdf")))
            read_pdf(str(_pdf(tmp_path, name="two.pdf")))
        assert len(list((tmp_path / "cache" / "pdf-text").iterdir())) == 2


def describe_cache_wiring():
    def it_buckets_every_pdf_under_one_subdirectory(tmp_path):
        with _cache_at(tmp_path):
            assert module.pdf_cache_dir("/abs/paper.pdf", 1.0) == (
                tmp_path / "cache" / "pdf-text"
            )

    def it_pins_the_decorator_configuration():
        # Reaching into cachetta's wrapper on purpose: these three settings are
        # the whole contract (bucket, key-per-arg-set, freshness), and reading
        # them back is what makes a silent upstream rename fail loudly.
        cache = module.extract._cache
        assert cache.path is module.pdf_cache_dir
        assert cache.hashed is True
        assert cache.duration == timedelta(days=365)
