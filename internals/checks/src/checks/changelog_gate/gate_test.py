from unittest import mock

from checks.changelog_gate.gate import run


def _run(changed, added=None, *, messages="feat: a change\n"):
    return run(
        "base",
        "head",
        changed_files=mock.Mock(return_value=changed),
        added_files=mock.Mock(return_value=added if added is not None else []),
        commit_messages=mock.Mock(return_value=messages),
    )


def describe_run():
    def bypasses_via_skip_changelog_line(capsys):
        changed = mock.Mock()
        rc = run(
            "base",
            "head",
            changed_files=changed,
            added_files=mock.Mock(),
            commit_messages=mock.Mock(return_value="x\n\nskip-changelog: internal"),
        )
        assert rc == 0
        assert "bypassing changelog enforcement" in capsys.readouterr().out
        changed.assert_not_called()

    def passes_when_no_package_source_changed(capsys):
        rc = _run(["README.md", "docs/guide.md"])
        assert rc == 0
        assert "No package source changed" in capsys.readouterr().out

    def passes_when_the_changed_package_has_a_fragment(capsys):
        rc = _run(
            ["packages/rust/src/lib.rs"],
            ["packages/rust/changelog.d/2026-07-13-fix.md"],
        )
        assert rc == 0

    def fails_when_a_changed_package_has_no_fragment(capsys):
        rc = _run(["packages/rust/src/lib.rs"])
        assert rc == 1
        out = capsys.readouterr().out
        assert "packages/rust has code changes" in out
        assert "packages/rust/changelog.d/YYYY-MM-DD-<slug>.md" in out

    def a_fragment_in_a_different_package_does_not_satisfy(capsys):
        rc = _run(
            ["packages/rust/src/lib.rs"],
            ["packages/ts/changelog.d/2026-07-13-fix.md"],
        )
        assert rc == 1
        assert "packages/rust has code changes" in capsys.readouterr().out

    def a_migrations_fragment_satisfies_the_gate(capsys):
        rc = _run(
            ["packages/rust/src/lib.rs"],
            ["packages/rust/migrations.d/2026-07-13-break.md"],
        )
        assert rc == 0

    def skips_a_package_whose_only_changes_are_exempt(capsys):
        rc = _run(["packages/rust/CHANGELOG.md", "packages/rust/tests/cli.rs"])
        assert rc == 0
        assert "No package source changed" not in capsys.readouterr().out

    def flags_a_malformed_fragment_filename(capsys):
        rc = _run(["packages/rust/changelog.d/notes.md"])
        assert rc == 1
        out = capsys.readouterr().out
        assert "fragment filenames must match" in out
        assert "packages/rust/changelog.d/notes.md" in out

    def a_malformed_fragment_fails_even_with_no_package_source(capsys):
        rc = _run(["packages/rust/changelog.d/notes.md", "README.md"])
        assert rc == 1
        assert "No package source changed" not in capsys.readouterr().out

    def reports_every_uncovered_package(capsys):
        rc = _run(["packages/rust/src/lib.rs", "packages/ts/src/x.ts"])
        assert rc == 1
        out = capsys.readouterr().out
        assert "packages/rust has code changes" in out
        assert "packages/ts has code changes" in out
