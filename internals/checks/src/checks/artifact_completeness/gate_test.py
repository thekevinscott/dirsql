"""Colocated unit tests for the artifact-completeness gate (#790)."""

from checks.artifact_completeness.gate import declared_targets, warn

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


def describe_warn():
    def it_writes_to_stderr(capsys):
        warn("boom")
        assert capsys.readouterr().err == "boom\n"
