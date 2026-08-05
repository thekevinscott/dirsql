"""Colocated unit tests for the artifact-completeness gate (#790)."""

from checks.artifact_completeness.gate import (
    declared_targets,
    missing,
    populated,
    read_config,
    run,
    subdirectories,
    warn,
)

CONFIG = {
    "package": [
        {"name": "dirsql-npm", "targets": ["linux-x64-gnu", {"triple": "darwin-arm64"}]},
        {"name": "dirsql-rust"},
    ]
}


def describe_declared_targets():
    def it_reads_plain_and_table_target_entries():
        assert declared_targets(CONFIG) == [
            *[("dirsql-npm", "linux-x64-gnu"), ("dirsql-npm", "darwin-arm64")]
        ]

    def it_skips_packages_that_declare_no_targets():
        assert declared_targets({"package": [{"name": "x"}]}) == []

    def it_skips_a_target_table_with_no_triple():
        assert declared_targets({"package": [{"name": "x", "targets": [{"runner": "r"}]}]}) == []

    def it_skips_a_package_with_no_name():
        assert declared_targets({"package": [{"targets": ["a"]}]}) == []

    def it_returns_empty_for_a_config_with_no_packages():
        assert declared_targets({}) == []


def describe_populated():
    def it_is_true_when_a_file_exists_at_any_depth():
        walk = lambda _d: [("d", ["sub"], []), ("d/sub", [], ["a.node"])]  # noqa: E731
        assert populated("d", walk) is True

    def it_is_false_for_a_tree_of_empty_directories():
        walk = lambda _d: [("d", ["sub"], []), ("d/sub", [], [])]  # noqa: E731
        assert populated("d", walk) is False


def full(_d):
    return [("x", [], ["f"])]


def empty(_d):
    return [("x", [], [])]


def describe_missing():
    def it_reports_a_target_with_no_matching_artifact():
        assert missing("dist", [("pkg", "aarch64")], ["pkg-x86_64"], full) == [
            "pkg / aarch64: no artifact directory matching *aarch64*"
        ]

    def it_reports_a_matching_artifact_that_is_empty():
        assert missing("dist", [("pkg", "t1")], ["pkg-t1"], empty) == [
            "pkg / t1: artifact present but empty (pkg-t1)"
        ]

    def it_accepts_a_populated_match():
        assert missing("dist", [("pkg", "t1")], ["pkg-t1"], full) == []

    def it_accepts_whatever_mode_segment_the_engine_inserts():
        # `pkg-napi-t1` and `pkg-t1` must both satisfy (pkg, t1); the engine's
        # segment rule is not ours to encode (#788).
        assert missing("dist", [("pkg", "t1")], ["pkg-napi-t1"], full) == []

    def it_requires_both_the_package_name_and_the_target_to_match():
        assert missing("dist", [("pkg", "t1")], ["other-t1", "pkg-t2"], full) == [
            "pkg / t1: no artifact directory matching *t1*"
        ]

    def it_accepts_when_any_one_of_several_matches_is_populated():
        walk = lambda d: empty(d) if d.endswith("pkg-t1") else full(d)  # noqa: E731
        assert missing("dist", [("pkg", "t1")], ["pkg-t1", "pkg-napi-t1"], walk) == []

    def it_reports_every_failing_pair():
        assert len(missing("dist", [("p", "a"), ("p", "b")], [], full)) == 2


def describe_subdirectories():
    def it_lists_only_directories_sorted(tmp_path):
        (tmp_path / "b").mkdir()
        (tmp_path / "a").mkdir()
        (tmp_path / "note.txt").write_text("x")
        assert subdirectories(str(tmp_path)) == ["a", "b"]

    def it_returns_empty_when_the_dist_dir_does_not_exist(tmp_path):
        assert subdirectories(str(tmp_path / "nope")) == []


def describe_read_config():
    def it_parses_a_toml_config(tmp_path):
        path = tmp_path / "c.toml"
        path.write_text('[[package]]\nname = "x"\n')
        assert read_config(str(path)) == {"package": [{"name": "x"}]}


def describe_warn():
    def it_writes_to_stderr(capsys):
        warn("boom")
        assert capsys.readouterr().err == "boom\n"


def drive(entries, walk=full, echo=None):
    return run(
        "dist",
        "c.toml",
        config=lambda _p: CONFIG,
        entries=lambda _d: entries,
        walk=walk,
        echo=echo or (lambda _line: None),
    )


def describe_run():
    def it_returns_zero_and_says_so_when_every_pair_is_present():
        lines = []
        code = drive(["dirsql-npm-linux-x64-gnu", "dirsql-npm-darwin-arm64"], echo=lines.append)
        assert code == 0
        assert lines == ["ok artifact-completeness: all 2 declared (package, target) pairs present"]

    def it_returns_one_and_names_the_pair_and_the_likely_cause():
        lines = []
        code = drive(["dirsql-npm-linux-x64-gnu"], echo=lines.append)
        assert code == 1
        assert lines[0] == (
            "incomplete artifact -- dirsql-npm / darwin-arm64: "
            "no artifact directory matching *darwin-arm64*"
        )
        assert "1 of 2 declared" in lines[1]
        assert "stages where the engine packages from" in lines[1]

    def it_takes_dist_dir_and_config_by_keyword():
        # `*` (not `/`) before the injected seams keeps both nameable.
        assert run(
            dist_dir="dist",
            config_path="c.toml",
            config=lambda _p: {},
            entries=lambda _d: [],
            walk=full,
            echo=lambda _line: None,
        ) == 0
