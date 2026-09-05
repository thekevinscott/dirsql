from . import no_rows_message as module


def describe_no_rows_message():
    def it_names_the_glob_and_the_root_when_no_file_matched():
        message = module.no_rows_message("docs/**/*.md", 0, "/work")
        assert message == (
            "no files matched 'docs/**/*.md' (searched from /work)"
        )

    def it_says_files_matched_but_none_were_embeddable():
        message = module.no_rows_message("docs/**/*.md", 3, "/work")
        assert message == (
            "'docs/**/*.md' matched 3 file(s), but none had text content to"
            " embed -- unreadable or not valid UTF-8 (searched from /work)"
        )

    def it_reports_a_none_count_as_nothing_matched():
        assert "no files matched" in module.no_rows_message("g", None, "/work")
