from unittest import mock

from checks.changelog_gate.gate import (
    MALFORMED_SKIP_CHANGELOG_MESSAGE,
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
        commit_messages = mock.Mock(return_value="feat: a change\n")
        rc = run(
            "base",
            "head",
            changed_files=changed_files,
            skip_trailers=skip_trailers,
            changelog_diff=changelog_diff,
            commit_messages=commit_messages,
        )
        assert rc == 1
        assert MISSING_CHANGELOG_MESSAGE in capsys.readouterr().err
        changelog_diff.assert_not_called()

    def names_a_malformed_skip_changelog_when_git_did_not_parse_it(capsys):
        # A `skip-changelog:` split out of the trailer block by a blank line:
        # git parses no trailer, so the gate must name the malformed attempt
        # instead of the generic "no entry" message.
        changed_files = mock.Mock(return_value=["packages/rust/src/lib.rs"])
        skip_trailers = mock.Mock(return_value="")
        changelog_diff = mock.Mock()
        commit_messages = mock.Mock(
            return_value="feat: a change\n\nskip-changelog: internal\n\nCo-Authored-By: x <x@y.z>\n"
        )
        rc = run(
            "base",
            "head",
            changed_files=changed_files,
            skip_trailers=skip_trailers,
            changelog_diff=changelog_diff,
            commit_messages=commit_messages,
        )
        assert rc == 1
        assert MALFORMED_SKIP_CHANGELOG_MESSAGE in capsys.readouterr().err
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

    def passes_when_a_changelog_fragment_is_added(capsys):
        changed_files = mock.Mock(
            return_value=[
                "packages/rust/src/lib.rs",
                "changelog.d/claude-my-branch-abc123.changed.md",
            ]
        )
        skip_trailers = mock.Mock(return_value="")
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
        assert "fragment" in out
        assert "changelog.d/claude-my-branch-abc123.changed.md" in out
        changelog_diff.assert_not_called()

    def the_fragment_dir_readme_alone_does_not_satisfy_the_gate(capsys):
        changed_files = mock.Mock(
            return_value=["packages/rust/src/lib.rs", "changelog.d/README.md"]
        )
        skip_trailers = mock.Mock(return_value="")
        changelog_diff = mock.Mock()
        commit_messages = mock.Mock(return_value="feat: a change\n")
        rc = run(
            "base",
            "head",
            changed_files=changed_files,
            skip_trailers=skip_trailers,
            changelog_diff=changelog_diff,
            commit_messages=commit_messages,
        )
        assert rc == 1
        assert MISSING_CHANGELOG_MESSAGE in capsys.readouterr().err

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
