"""Colocated test for the package barrel.

`dirsql/__init__.py` re-exports the public API surface from the implementation
modules; this asserts the barrel exposes exactly the declared public names and
that each resolves to a real value. Mirrors the TypeScript `index.test.ts`
barrel test, so the barrel is covered by a colocated test rather than a
testing-conventions exemption. Imports are relative (`from . import ...`) so the
barrel under test is the unit, not an external collaborator.
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
