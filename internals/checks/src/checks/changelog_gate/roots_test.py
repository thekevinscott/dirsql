from checks.changelog_gate.roots import ROOTS


def describe_roots():
    def it_is_exactly_packages_and_plugins():
        # Pinned literally: this tuple is the whole scope of the gate, and the
        # `internals/` tree is out of it only because it is absent here.
        assert ROOTS == ("packages", "plugins")
