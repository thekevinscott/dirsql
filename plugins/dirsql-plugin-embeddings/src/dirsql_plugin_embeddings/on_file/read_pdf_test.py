"""Colocated unit test for `read_pdf` (isolation).

`PdfReader` is mocked: this unit owns page traversal and joining, not pypdf's
parsing. Real extraction from a real PDF is covered by the integration and e2e
tiers once `read_pdf` is wired to the console script.
"""

from unittest import mock

from . import read_pdf as module
from .read_pdf import read_pdf


def _reader(*pages):
    return mock.Mock(pages=[mock.Mock(**{"extract_text.return_value": p}) for p in pages])


def describe_read_pdf():
    def it_extracts_text_from_a_single_page():
        with mock.patch.object(module, "PdfReader", return_value=_reader("hello")):
            assert read_pdf("/abs/paper.pdf") == "hello"

    def it_joins_every_page_in_order():
        with mock.patch.object(module, "PdfReader", return_value=_reader("one", "two", "three")):
            assert read_pdf("/abs/paper.pdf") == "one\ntwo\nthree"

    def it_opens_the_path_it_is_given():
        with mock.patch.object(
            module, "PdfReader", return_value=_reader("x")
        ) as reader:
            read_pdf("/abs/paper.pdf")
        reader.assert_called_once_with("/abs/paper.pdf")

    def it_returns_empty_text_for_a_scanned_page_rather_than_raising():
        # pypdf yields '' for an image-only page; that is a file with no text,
        # not a failure, and matches how an empty .md already behaves.
        with mock.patch.object(module, "PdfReader", return_value=_reader("")):
            assert read_pdf("/abs/scan.pdf") == ""

    def it_propagates_an_extraction_failure():
        boom = ValueError("corrupt xref")
        with mock.patch.object(module, "PdfReader", side_effect=boom):
            try:
                read_pdf("/abs/broken.pdf")
            except ValueError as exc:
                assert exc is boom
            else:
                raise AssertionError("expected the pypdf error to propagate")
