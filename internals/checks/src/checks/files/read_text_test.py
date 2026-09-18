from unittest import mock

from checks.files.read_text import read_text


def describe_read_text():
    def it_reads_a_file_as_utf8():
        with mock.patch(
            "checks.files.read_text.open", mock.mock_open(read_data="jobs: {}\n")
        ) as opened:
            assert read_text("wf.yml") == "jobs: {}\n"
        opened.assert_called_once_with("wf.yml", encoding="utf-8")
