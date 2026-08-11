from unittest.mock import MagicMock, patch

from . import cli


def describe_main():
    def it_is_a_group_with_exactly_the_worker_subcommand():
        assert set(cli.main.commands) == {"worker"}

    def it_registers_the_worker_command_object():
        assert cli.main.commands["worker"] is cli.worker


def describe_worker_command():
    def it_runs_the_worker_over_stdin_and_stdout():
        fake_sys = MagicMock()
        with patch.object(cli, "configure") as configure:
            with patch.object(cli, "Worker") as worker_class:
                with patch.object(cli, "sys", fake_sys):
                    cli.worker.callback()
        configure.assert_called_once_with()
        worker_class.assert_called_once_with()
        worker_class.return_value.serve.assert_called_once_with(
            fake_sys.stdin, fake_sys.stdout
        )

    def it_configures_progress_before_serving():
        order = []
        with patch.object(
            cli, "configure", side_effect=lambda: order.append("configure")
        ):
            with patch.object(cli, "Worker") as worker_class:
                worker_class.return_value.serve.side_effect = (
                    lambda *args: order.append("serve")
                )
                with patch.object(cli, "sys", MagicMock()):
                    cli.worker.callback()
        assert order == ["configure", "serve"]
