from . import normalize_glob as module


def describe_path_prefixes():
    def it_is_exactly_the_four_prefixes_the_core_rescues():
        assert module.PATH_PREFIXES == ("./", "../", "/", "~/")


def describe_normalize_glob():
    def it_prefixes_a_bare_relative_glob_with_dot_slash():
        assert module.normalize_glob("**/*.md") == "./**/*.md"

    def it_keeps_a_dot_slash_glob():
        assert module.normalize_glob("./notes/*.txt") == "./notes/*.txt"

    def it_keeps_a_parent_relative_glob():
        assert module.normalize_glob("../notes/*.txt") == "../notes/*.txt"

    def it_keeps_an_absolute_glob():
        assert module.normalize_glob("/tmp/notes/*.txt") == "/tmp/notes/*.txt"

    def it_keeps_a_home_glob():
        assert module.normalize_glob("~/notes/*.txt") == "~/notes/*.txt"
