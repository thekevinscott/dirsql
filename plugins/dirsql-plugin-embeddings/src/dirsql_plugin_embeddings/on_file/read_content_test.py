"""Colocated unit test for `read_content` (isolation).

Both readers are mocked: this unit owns the extension dispatch, not either
read. Real extraction from a real file is covered by the integration and e2e
tiers.
"""

from unittest import mock

from . import read_content as module
from .read_content import read_content


def _dispatch(path):
    """Call `read_content` with both readers mocked; return (result, mocks)."""
    with (
        mock.patch.object(module, "read_text", return_value="text body") as read_text,
        mock.patch.object(module, "read_pdf", return_value="pdf body") as read_pdf,
    ):
        return read_content(path), read_text, read_pdf


def describe_read_content():
    def it_reads_a_markdown_file_as_text():
        result, read_text, read_pdf = _dispatch("/abs/note.md")
        assert result == "text body"
        read_text.assert_called_once_with("/abs/note.md")
        read_pdf.assert_not_called()

    def it_reads_an_extensionless_file_as_text():
        result, read_text, read_pdf = _dispatch("/abs/LICENSE")
        assert result == "text body"
        read_text.assert_called_once_with("/abs/LICENSE")
        read_pdf.assert_not_called()

    def it_reads_an_extension_sorting_after_pdf_as_text():
        # `.txt` sorts above `.pdf`: the dispatch is an equality test, not an
        # ordering one, so every other extension must still read as text.
        result, read_text, read_pdf = _dispatch("/abs/notes.txt")
        assert result == "text body"
        read_text.assert_called_once_with("/abs/notes.txt")
        read_pdf.assert_not_called()

    def it_reads_a_pdf_with_read_pdf():
        result, read_text, read_pdf = _dispatch("/abs/paper.pdf")
        assert result == "pdf body"
        read_pdf.assert_called_once_with("/abs/paper.pdf")
        read_text.assert_not_called()

    def it_routes_an_uppercase_pdf_extension_to_read_pdf():
        result, read_text, read_pdf = _dispatch("/abs/PAPER.PDF")
        assert result == "pdf body"
        read_pdf.assert_called_once_with("/abs/PAPER.PDF")
        read_text.assert_not_called()

    def it_routes_a_mixed_case_pdf_extension_to_read_pdf():
        result, read_text, read_pdf = _dispatch("/abs/paper.Pdf")
        assert result == "pdf body"
        read_pdf.assert_called_once_with("/abs/paper.Pdf")
        read_text.assert_not_called()

    def it_matches_the_extension_not_the_stem():
        # `.pdf` mid-name is not the extension; only the trailing one routes.
        result, read_text, read_pdf = _dispatch("/abs/about.pdf.md")
        assert result == "text body"
        read_text.assert_called_once_with("/abs/about.pdf.md")
        read_pdf.assert_not_called()
