"""Colocated unit tests for declared-deps' import extraction (#782)."""

from checks.declared_deps.top_level_imports import top_level_imports


def describe_top_level_imports():
    def it_reads_plain_and_dotted_imports():
        assert top_level_imports("import os.path\nimport click\n") == {"os", "click"}

    def it_reads_from_imports_by_their_root_module():
        assert top_level_imports("from bin_shim.core import main\n") == {"bin_shim"}

    def it_ignores_relative_imports_which_are_always_first_party():
        assert top_level_imports("from . import sibling\nfrom .a.b import c\n") == set()

    def it_ignores_a_bare_relative_import_with_no_module():
        assert top_level_imports("from .. import x\n") == set()
