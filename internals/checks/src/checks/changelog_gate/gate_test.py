from unittest import mock

from checks.changelog_gate.gate import (
    MALFORMED_SKIP_CHANGELOG_MESSAGE,
    run,
)


def _run(files, *, trailers="", messages="feat: a change\n"):
    return run(
        "base",
        "head",
        changed_files=mock.Mock(return_value=files),
        skip_trailers=mock.Mock(return_value=trailers),
        commit_messages=mock.Mock(return_value=messages),
    )


def describe_run():
    def skips_when_no_sdk_code_changed(capsys):
        skip_trailers = mock.Mock()
        rc = run(
            "base",
            "head",
            changed_files=mock.Mock(return_value=["README.md"]),
            skip_trailers=skip_trailers,
        )
        assert rc == 0
        assert "No SDK code changes detected" in capsys.readouterr().out
        skip_trailers.assert_not_called()

    def bypasses_via_skip_changelog_trailer(capsys):
        rc = _run(["packages/rust/src/lib.rs"], trailers="internal refactor\n")
        assert rc == 0
        out = capsys.readouterr().out
        assert "Bypassing changelog check" in out
        assert "internal refactor" in out

    def passes_when_the_changed_package_has_a_fragment(capsys):
        rc = _run(
            [
                "packages/rust/src/lib.rs",
                "packages/rust/changelog.d/2026-07-13-fix-race.md",
            ]
        )
        assert rc == 0
        out = capsys.readouterr().out
        assert "fragment(s) present for: rust" in out

    def passes_when_every_changed_package_has_its_own_fragment(capsys):
        rc = _run(
            [
                "packages/rust/src/lib.rs",
                "packages/ts/src/table.ts",
                "packages/rust/changelog.d/2026-07-13-core.md",
                "packages/ts/changelog.d/2026-07-13-ts.md",
            ]
        )
        assert rc == 0
        assert "rust, ts" in capsys.readouterr().out

    def fails_when_a_changed_package_has_no_fragment(capsys):
        rc = _run(["packages/rust/src/lib.rs"])
        assert rc == 1
        err = capsys.readouterr().err
        assert "packages/rust/changelog.d/YYYY-MM-DD-<slug>.md" in err
        # Singular, not "1 packages" -- pins the plural to exactly-one.
        assert "1 package:" in err
        assert "1 packages" not in err

    def an_extra_fragment_for_an_unchanged_package_is_harmless(capsys):
        # rust source changed + covered; a stray ts fragment (ts unchanged)
        # must not turn the gate red -- the requirement is set difference
        # (changed - covered), not symmetric difference.
        rc = _run(
            [
                "packages/rust/src/lib.rs",
                "packages/rust/changelog.d/2026-07-13-core.md",
                "packages/ts/changelog.d/2026-07-13-extra.md",
            ]
        )
        assert rc == 0
        assert "fragment(s) present for: rust" in capsys.readouterr().out

    def names_only_the_uncovered_package_when_another_is_covered(capsys):
        rc = _run(
            [
                "packages/rust/src/lib.rs",
                "packages/ts/src/table.ts",
                "packages/rust/changelog.d/2026-07-13-core.md",
            ]
        )
        assert rc == 1
        err = capsys.readouterr().err
        assert "packages/ts/changelog.d/YYYY-MM-DD-<slug>.md" in err
        assert "packages/rust/changelog.d" not in err

    def lists_all_uncovered_packages_with_a_plural_message(capsys):
        rc = _run(["packages/rust/src/lib.rs", "packages/python/dirsql/table.py"])
        assert rc == 1
        err = capsys.readouterr().err
        assert "2 packages: python, rust" in err

    def rejects_a_malformed_fragment_filename(capsys):
        rc = _run(
            [
                "packages/rust/src/lib.rs",
                "packages/rust/changelog.d/notes.md",
            ]
        )
        assert rc == 1
        err = capsys.readouterr().err
        assert "filename is malformed" in err
        assert "packages/rust/changelog.d/notes.md" in err

    def the_dir_readme_alone_does_not_satisfy_the_gate(capsys):
        rc = _run(["packages/rust/src/lib.rs", "packages/rust/changelog.d/README.md"])
        assert rc == 1
        assert "no changelog fragment was added" in capsys.readouterr().err

    def names_a_malformed_skip_changelog_when_git_did_not_parse_it(capsys):
        rc = _run(
            ["packages/rust/src/lib.rs"],
            messages="feat: x\n\nskip-changelog: internal\n\nCo-Authored-By: a <a@b.c>\n",
        )
        assert rc == 1
        assert MALFORMED_SKIP_CHANGELOG_MESSAGE in capsys.readouterr().err
