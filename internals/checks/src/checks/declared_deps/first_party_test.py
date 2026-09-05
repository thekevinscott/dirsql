"""Colocated unit tests for declared-deps' first-party name discovery (#782)."""

from checks.declared_deps.first_party import first_party


def describe_first_party():
    def it_names_the_scanned_dir_and_everything_directly_inside_it():
        assert first_party("src/checks", lambda _d: ["changelog.py", "preflight"]) == {
            *["checks", "changelog", "preflight"]
        }

    def it_ignores_a_trailing_slash_on_the_scanned_dir():
        assert "checks" in first_party("src/checks/", lambda _d: [])

    def it_lists_the_real_directory_by_default():
        assert first_party.__defaults__[0].__name__ == "listdir"
