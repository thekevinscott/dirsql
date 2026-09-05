"""Colocated unit tests for the (package, target) artifact invariant (#790)."""

from checks.artifact_completeness.missing import built_packages, missing, populated


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


def describe_built_packages():
    def it_names_a_package_with_any_artifact():
        assert built_packages([("pkg", "t1")], ["pkg-main"]) == {"pkg"}

    def it_omits_a_package_the_plan_did_not_build():
        assert built_packages([("pkg", "t1")], ["other-t1"]) == set()


def describe_missing():
    def it_skips_a_package_the_plan_did_not_build_at_all():
        # The precheck matrix only builds packages the PR touched; a docs-only
        # PR legitimately produces nothing and must not fail.
        assert missing("dist", [("pkg", "t1")], [], full) == []

    def it_keeps_checking_packages_after_one_it_skipped():
        # `unbuilt` sorts before `pkg`, so a `break` on the skip would hide the
        # real failure behind a package the plan never built.
        assert missing("dist", [("unbuilt", "t1"), ("pkg", "t1")], ["pkg-main"], full) == [
            "pkg / t1: no artifact directory matching *t1*"
        ]

    def it_still_fails_a_built_package_missing_a_target():
        # #788's signature: the `main` row shipped, the platform rows did not.
        assert missing("dist", [("pkg", "t1")], ["pkg-main"], full) == [
            "pkg / t1: no artifact directory matching *t1*"
        ]

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
        assert len(missing("dist", [("p", "t1"), ("p", "t2")], ["p-main"], full)) == 2

    def it_joins_the_dist_dir_onto_each_candidate_before_walking():
        seen = []
        walk = lambda d: (seen.append(d), full(d))[1]  # noqa: E731
        missing("dist", [("pkg", "t1")], ["pkg-t1"], walk)
        assert seen == ["dist/pkg-t1"]
