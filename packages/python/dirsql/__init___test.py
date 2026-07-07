"""Colocated test for the package barrel: asserts it exposes exactly the
declared public names and that each resolves to a real value. Imports are
relative so the barrel under test is the unit, not an external collaborator.
"""

from . import DirSQL, RowEvent, Table, __all__, __version__


def describe_public_barrel():
    def it_exposes_exactly_the_declared_public_names():
        assert set(__all__) == {"DirSQL", "Table", "RowEvent", "__version__"}

    def it_re_exports_the_runtime_values():
        assert DirSQL is not None
        assert Table is not None
        assert RowEvent is not None
        assert isinstance(__version__, str)
