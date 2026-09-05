"""Colocated unit tests for package-root resolution (#781)."""

from checks.preflight.package_root import MANIFESTS, package_root


def has_manifest(path: str) -> bool:
    return path == "packages/python/pyproject.toml"


def describe_package_root():
    def it_walks_up_to_the_nearest_manifest():
        assert package_root("packages/python/dirsql", has_manifest) == "packages/python"

    def it_returns_the_source_itself_when_it_holds_the_manifest():
        assert package_root("packages/python", has_manifest) == "packages/python"

    def it_falls_back_to_the_repo_root_when_no_ancestor_has_one():
        assert package_root("packages/rust", lambda _path: False) == "."

    def it_recognises_a_node_or_rust_manifest_too():
        assert package_root("packages/ts/src", lambda p: p == "packages/ts/package.json") == (
            "packages/ts"
        )
        assert package_root("packages/rust/src", lambda p: p == "packages/rust/Cargo.toml") == (
            "packages/rust"
        )


def describe_manifests():
    def it_is_one_manifest_name_per_supported_ecosystem():
        assert MANIFESTS == ("pyproject.toml", "package.json", "Cargo.toml")
