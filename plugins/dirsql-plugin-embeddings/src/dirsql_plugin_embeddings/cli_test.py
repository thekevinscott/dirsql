from unittest.mock import MagicMock, patch

import pytest

from . import cli


def describe_main():
    def it_runs_the_worker_over_stdin_and_stdout():
        fake_sys = MagicMock()
        with patch.object(cli, "configure") as configure:
            with patch.object(cli, "Worker") as worker_class:
                with patch.object(cli, "sys", fake_sys):
                    code = cli.main(["worker"])
        configure.assert_called_once_with()
        worker_class.assert_called_once_with()
        worker_class.return_value.serve.assert_called_once_with(
            fake_sys.stdin, fake_sys.stdout
        )
        assert code == 0

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
                    cli.main(["worker"])
        assert order == ["configure", "serve"]

    def it_requires_a_subcommand():
        with patch.object(cli, "Worker") as worker_class:
            with pytest.raises(SystemExit) as excinfo:
                cli.main([])
        worker_class.return_value.serve.assert_not_called()
        assert excinfo.value.code == 2

    def it_rejects_an_unknown_subcommand():
        with patch.object(cli, "Worker") as worker_class:
            with pytest.raises(SystemExit) as excinfo:
                cli.main(["bogus"])
        worker_class.return_value.serve.assert_not_called()
        assert excinfo.value.code == 2
