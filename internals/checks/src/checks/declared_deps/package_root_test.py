"""Colocated unit tests for the declared-deps package-root walk (#782)."""

from checks.declared_deps.package_root import package_root


def describe_package_root():
    def it_walks_up_to_the_nearest_pyproject():
        assert package_root("a/b/c", lambda p: p == "a/pyproject.toml") == "a"

    def it_falls_back_to_the_repo_root_when_no_ancestor_has_one():
        assert package_root("a/b", lambda _p: False) == "."

    def it_probes_the_real_filesystem_by_default():
        assert package_root.__defaults__[0].__name__ == "exists"
