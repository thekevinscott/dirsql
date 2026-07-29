"""Colocated test for the package barrel: asserts it exposes exactly the
declared public name and that it resolves to the entry point the console
script targets. Imports are relative so the barrel under test is the unit, not
an external collaborator.
"""

from . import __all__, on_file


def describe_on_file_barrel():
    def it_exposes_exactly_the_declared_public_names():
        assert set(__all__) == {"on_file"}

    def it_re_exports_the_entry_point_callable():
        assert callable(on_file)
