"""Colocated unit tests for the declared-deps name vocabulary (#782)."""

from checks.declared_deps.gate import normalize, providers, requirement_name, warn


def describe_normalize():
    def it_lowercases_and_folds_underscores_to_dashes():
        assert normalize("Bin_Shim") == "bin-shim"


def describe_requirement_name():
    def it_strips_the_version_specifier():
        assert requirement_name("click>=8.1") == "click"

    def it_strips_extras_markers_and_exclusions():
        assert [requirement_name(s) for s in ("a[x]", "b!=1", "c;python<4", "d ==1", "e~=2")] == [
            *["a", "b", "c", "d", "e"]
        ]


def describe_providers():
    def it_maps_an_import_name_to_its_distributions():
        assert providers("yaml", {"yaml": ["PyYAML"]}) == {"pyyaml"}

    def it_falls_back_to_the_import_name_when_nothing_provides_it():
        assert providers("bin_shim", {}) == {"bin-shim"}


def describe_warn():
    def it_writes_to_stderr(capsys):
        warn("boom")
        assert capsys.readouterr().err == "boom\n"
