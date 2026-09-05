"""Integration tests for the artifact-completeness check against a real tree.

Exercises `gate.run` with its default collaborators (the real filesystem, a real
TOML parse) over a real scratch artifact directory, never the packaged
`dirsql-checks` CLI (that's the e2e tier).

The reproduction is #788 exactly: the npm platform build rows uploaded nothing,
so the run carried no `dirsql-npm-<triple>` artifact at all. Every build job
reported success -- `upload-artifact` only warns on an empty glob -- and the
failure surfaced after merge, at publish, as `missing artifact directory`.
"""

from __future__ import annotations

from checks.artifact_completeness.run import run

CONFIG = """
[[package]]
name = "dirsql-npm"
targets = ["linux-x64-gnu", "darwin-arm64"]

[[package]]
name = "dirsql-rust"
"""


def write_config(tmp_path):
    path = tmp_path / "putitoutthere.toml"
    path.write_text(CONFIG)
    return str(path)


def stage(root, artifacts):
    root.mkdir(parents=True, exist_ok=True)
    for name, files in artifacts.items():
        d = root / name
        d.mkdir()
        for f in files:
            (d / f).write_bytes(b"x")
    return str(root)


def describe_run_against_a_real_artifact_tree():
    def it_passes_when_every_declared_target_has_a_populated_artifact(tmp_path):
        dist = stage(
            tmp_path / "dist",
            {
                "dirsql-npm-linux-x64-gnu": ["dirsql.linux-x64-gnu.node"],
                "dirsql-npm-darwin-arm64": ["dirsql.darwin-arm64.node"],
            },
        )
        assert run(dist, write_config(tmp_path)) == 0

    def it_fails_when_a_targets_artifact_is_missing_entirely(tmp_path, capsys):
        # #788: the upload matched nothing, so no artifact was created at all.
        dist = stage(
            tmp_path / "dist",
            {"dirsql-npm-linux-x64-gnu": ["dirsql.linux-x64-gnu.node"]},
        )
        assert run(dist, write_config(tmp_path)) == 1
        err = capsys.readouterr().err
        assert "dirsql-npm" in err and "darwin-arm64" in err

    def it_fails_when_an_artifact_exists_but_is_empty(tmp_path, capsys):
        dist = stage(
            tmp_path / "dist",
            {
                "dirsql-npm-linux-x64-gnu": ["dirsql.linux-x64-gnu.node"],
                "dirsql-npm-darwin-arm64": [],
            },
        )
        assert run(dist, write_config(tmp_path)) == 1
        assert "darwin-arm64" in capsys.readouterr().err

    def it_ignores_packages_that_declare_no_targets(tmp_path):
        dist = stage(
            tmp_path / "dist",
            {
                "dirsql-npm-linux-x64-gnu": ["a.node"],
                "dirsql-npm-darwin-arm64": ["b.node"],
            },
        )
        assert run(dist, write_config(tmp_path)) == 0

    def it_matches_whatever_mode_segment_the_engine_uses(tmp_path):
        # The engine's suffix rule changed under us once already (#788): two
        # build rows gave `dirsql-npm-napi-<triple>`, one gives
        # `dirsql-npm-<triple>`. Matching on package name + target -- both
        # declared in OUR config -- survives that; matching the full engine
        # convention would be a second copy of the rule that broke.
        dist = stage(
            tmp_path / "dist",
            {
                "dirsql-npm-napi-linux-x64-gnu": ["a.node"],
                "dirsql-npm-napi-darwin-arm64": ["b.node"],
            },
        )
        assert run(dist, write_config(tmp_path)) == 0
