"""Colocated unit tests for the declared-deps source walk (#782)."""

from checks.declared_deps.source_files import source_files


def describe_source_files():
    def it_collects_python_files_recursively_and_sorted():
        walk = lambda _root: [("src", [], ["b.py", "a.py", "notes.md"]), ("src/sub", [], ["c.py"])]  # noqa: E731
        assert source_files("src", walk) == ["src/a.py", "src/b.py", "src/sub/c.py"]

    def it_walks_the_real_filesystem_by_default():
        assert source_files.__defaults__[0].__name__ == "walk"
