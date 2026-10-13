"""Hermetic test that the suite's glob cases carry distinct names."""

from collections import Counter

from tests.e2e import (
    glob_speed_dotfiles_test,
    glob_speed_recursive_test,
    glob_speed_symlink_test,
)


def case_names():
    modules = (
        glob_speed_recursive_test,
        glob_speed_symlink_test,
        glob_speed_dotfiles_test,
    )
    return [case[0] for module in modules for case in module.CASES]


def describe_glob_case_names():
    def it_gives_every_case_its_own_name():
        repeated = [n for n, count in Counter(case_names()).items() if count > 1]
        assert repeated == []
