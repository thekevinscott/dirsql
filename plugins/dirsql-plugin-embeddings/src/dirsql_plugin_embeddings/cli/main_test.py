from . import main


def describe_main():
    def it_is_a_group_with_exactly_the_worker_subcommand():
        assert set(main.main.commands) == {"worker"}

    def it_registers_the_worker_command_module():
        assert main.main.commands["worker"] is main.worker
