from unittest import mock

from checks.changelog_gate.gate import (
    MISSING_CHANGELOG_MESSAGE,
    NO_ADDED_CONTENT_MESSAGE,
    run,
)


def describe_run():
    def skips_when_no_sdk_code_changed(capsys):
        changed_files = mock.Mock(return_value=["README.md"])
        skip_trailers = mock.Mock()
        changelog_diff = mock.Mock()
        rc = run(
            "base",
            "head",
            changed_files=changed_files,
            skip_trailers=skip_trailers,
            changelog_diff=changelog_diff,
        )
        assert rc == 0
        assert "No SDK code changes detected" in capsys.readouterr().out
        skip_trailers.assert_not_called()
        changelog_diff.assert_not_called()

    def bypasses_via_skip_changelog_trailer(capsys):
        changed_files = mock.Mock(return_value=["packages/rust/src/lib.rs"])
        skip_trailers = mock.Mock(return_value="internal refactor\n")
        changelog_diff = mock.Mock()
        rc = run(
            "base",
            "head",
            changed_files=changed_files,
            skip_trailers=skip_trailers,
            changelog_diff=changelog_diff,
        )
        assert rc == 0
        out = capsys.readouterr().out
        assert "Bypassing CHANGELOG check" in out
        assert "internal refactor" in out
        changelog_diff.assert_not_called()

    def fails_when_changelog_not_touched(capsys):
        changed_files = mock.Mock(return_value=["packages/rust/src/lib.rs"])
        skip_trailers = mock.Mock(return_value="")
        changelog_diff = mock.Mock()
        rc = run(
            "base",
            "head",
            changed_files=changed_files,
            skip_trailers=skip_trailers,
            changelog_diff=changelog_diff,
        )
        assert rc == 1
        assert MISSING_CHANGELOG_MESSAGE in capsys.readouterr().err
        changelog_diff.assert_not_called()

    def fails_when_changelog_touched_with_no_added_content(capsys):
        changed_files = mock.Mock(
            return_value=["packages/rust/src/lib.rs", "CHANGELOG.md"]
        )
        skip_trailers = mock.Mock(return_value="")
        changelog_diff = mock.Mock(return_value="+++ b/CHANGELOG.md\n")
        rc = run(
            "base",
            "head",
            changed_files=changed_files,
            skip_trailers=skip_trailers,
            changelog_diff=changelog_diff,
        )
        assert rc == 1
        assert NO_ADDED_CONTENT_MESSAGE in capsys.readouterr().err

    def passes_when_changelog_has_added_content(capsys):
        changed_files = mock.Mock(
            return_value=["packages/rust/src/lib.rs", "CHANGELOG.md"]
        )
        skip_trailers = mock.Mock(return_value="")
        changelog_diff = mock.Mock(return_value="+++ b/CHANGELOG.md\n+- new entry\n")
        rc = run(
            "base",
            "head",
            changed_files=changed_files,
            skip_trailers=skip_trailers,
            changelog_diff=changelog_diff,
        )
        assert rc == 0
        assert "updated with 1 added line(s). OK." in capsys.readouterr().out
