from checks.npm_addon_version.addon_names import addon_names


def describe_addon_names():
    def filters_to_addons_sorted():
        names = ["b.node", "package.json", "a.node"]
        assert addon_names(names) == ["a.node", "b.node"]

    def empty_when_no_addons():
        assert addon_names(["dirsql.whl", "notes.txt"]) == []
