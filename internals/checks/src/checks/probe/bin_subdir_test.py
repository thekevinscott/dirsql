import os

from checks.probe.bin_subdir import bin_subdir


def describe_bin_subdir():
    def windows_uses_scripts():
        assert bin_subdir("nt") == "Scripts"

    def posix_uses_bin():
        assert bin_subdir("posix") == "bin"

    def unknown_platform_falls_back_to_bin():
        assert bin_subdir("java") == "bin"

    def it_defaults_to_the_running_platform():
        assert bin_subdir() == bin_subdir(os.name)
