from . import cli


def describe_main():
    def it_is_a_group_with_exactly_the_worker_subcommand():
        assert set(cli.main.commands) == {"worker"}

    def it_registers_the_worker_subpackage_command():
        assert cli.main.commands["worker"] is cli.worker
