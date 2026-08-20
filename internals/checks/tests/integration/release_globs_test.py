"""Integration tests for the release-globs check against real files.

Exercises `gate.run` with its default collaborators (a real `tomllib` parse of a
real `putitoutthere.toml`, a real PyYAML parse of a real workflow) rather than
the packaged `dirsql-checks` CLI.

The reproduction is #944 exactly: `packages/rust/**` in the publish globs while
`release-ci.yml` excludes `packages/rust/changelog.d/**`, so a fragment-only PR
merges without ever running the build matrix and then republishes all three
packages. Release run 371 shipped that way off two `testing-conventions.toml`
files, which is the same shape one directory shallower.
"""

from __future__ import annotations

import pytest

from checks.release_globs.decide import carve_out
from checks.release_globs.gate import run

WORKFLOW = """\
name: Release CI
on:
  pull_request:
    branches: [main]
    paths:
{paths}
jobs:
  precheck:
    uses: thekevinscott/putitoutthere/.github/workflows/build.yml@v0
"""

EXCLUSIONS = [
    "packages/rust/**",
    "!packages/rust/changelog.d/**",
    "!packages/rust/migrations.d/**",
    "!packages/rust/e2e-attestations/**",
    "!packages/rust/testing-conventions.toml",
    "Cargo.toml",
]


def write(tmp_path, globs, paths=EXCLUSIONS):
    config = tmp_path / "putitoutthere.toml"
    entries = "\n".join(f'  "{glob}",' for glob in globs)
    config.write_text(
        f'[putitoutthere]\nversion = 1\n\n[[package]]\nname = "dirsql-rust"\n'
        f'kind = "crates"\npath = "packages/rust"\nglobs = [\n{entries}\n]\n'
    )
    workflow = tmp_path / "release-ci.yml"
    workflow.write_text(WORKFLOW.format(paths="\n".join(f"      - '{p}'" for p in paths)))
    return str(config), str(workflow)


def describe_run_against_real_config_files():
    def it_fails_on_the_944_mismatch(tmp_path, capsys):
        code = run(*write(tmp_path, ["packages/rust/**", "Cargo.toml", "Cargo.lock"]))
        assert code == 1
        out = capsys.readouterr().out
        assert "::error::dirsql-rust: publish globs for packages/rust" in out
        assert "packages/rust/!(changelog.d|migrations.d|e2e-attestations)/**" in out

    def it_passes_once_the_publish_globs_carve_the_same_paths_out(tmp_path, capsys):
        globs = [*carve_out("packages/rust"), "Cargo.toml", "Cargo.lock"]
        assert run(*write(tmp_path, globs)) == 0
        assert "ok release-globs" in capsys.readouterr().out

    def it_still_fails_when_the_precheck_skips_a_path_publishing_keeps(tmp_path, capsys):
        # The asymmetry #944 is about, in its general form: any path the build
        # precheck declines to build but the release still ships.
        globs = [*carve_out("packages/rust"), "Cargo.toml"]
        paths = [*EXCLUSIONS, "!packages/rust/benches/**"]
        assert run(*write(tmp_path, globs, paths)) == 1
        assert "release-ci.yml excludes" in capsys.readouterr().out

    @pytest.mark.parametrize("negation", ["!packages/rust/changelog.d/**", "!packages/rust/**"])
    def it_rejects_a_leading_bang_negation_in_the_publish_globs(tmp_path, capsys, negation):
        # Verified against putitoutthere @v0 (8f58767): `matchesAny` ORs the
        # globs, so this neither subtracts nor stays inert -- under minimatch it
        # matches every path outside the named subtree.
        globs = [*carve_out("packages/rust"), negation]
        assert run(*write(tmp_path, globs)) == 1
        assert "leading-`!` negation" in capsys.readouterr().out
