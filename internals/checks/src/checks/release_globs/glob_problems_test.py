"""Colocated unit tests for the publish-glob problem messages (#944)."""

from checks.release_globs.glob_problems import glob_problems

RUST = [
    "packages/rust/!(testing-conventions.toml)",
    "packages/rust/!(changelog.d|migrations.d|e2e-attestations)/**",
]


def describe_glob_problems():
    def it_accepts_a_package_whose_globs_carve_out_the_non_shipping_paths():
        assert glob_problems([{"name": "dirsql-rust", "globs": [*RUST, "Cargo.toml"]}]) == []

    def it_rejects_a_bare_recursive_subtree_glob():
        problems = glob_problems([{"name": "dirsql-rust", "globs": ["packages/rust/**"]}])
        assert len(problems) == 1
        assert "publish globs for packages/rust are packages/rust/**" in problems[0]
        assert RUST[1] in problems[0]

    def it_rejects_a_carve_out_missing_the_root_file_pattern():
        problems = glob_problems([{"name": "dirsql-rust", "globs": [RUST[1]]}])
        assert len(problems) == 1
        assert RUST[0] in problems[0]

    def it_rejects_a_leading_bang_negation_and_explains_the_extglob_form():
        problems = glob_problems(
            [{"name": "dirsql-rust", "globs": [*RUST, "!packages/rust/changelog.d/**"]}]
        )
        assert len(problems) == 1
        assert "leading-`!` negation" in problems[0]
        assert "matches every path outside its own subtree" in problems[0]
        # The suggestion has to be the subdirectory pattern -- the fragment dirs
        # the author was reaching for live there, not in the root-file pattern.
        assert '"<root>/!(changelog.d|migrations.d|e2e-attestations)/**"' in problems[0]

    def it_names_every_non_shipping_entry_in_the_mismatch_message():
        problems = glob_problems([{"name": "dirsql-rust", "globs": ["packages/rust/**"]}])
        assert "changelog.d, migrations.d, e2e-attestations, testing-conventions.toml" in (
            problems[0]
        )

    def it_reports_every_root_a_package_globs():
        problems = glob_problems(
            [{"name": "dirsql-py", "globs": ["packages/python/**", "packages/rust/**"]}]
        )
        assert len(problems) == 2

    def it_reports_every_package():
        problems = glob_problems(
            [{"name": "a", "globs": ["packages/rust/**"]}, {"name": "b", "globs": ["packages/ts/**"]}]
        )
        assert len(problems) == 2

    def it_names_an_unnamed_package_rather_than_crashing():
        assert glob_problems([{"globs": ["packages/rust/**"]}])[0].startswith("<unnamed>:")

    def it_accepts_a_package_declaring_no_globs():
        assert glob_problems([{"name": "a"}]) == []

    def it_accepts_an_empty_package_list():
        assert glob_problems([]) == []
