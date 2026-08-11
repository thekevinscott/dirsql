from unittest.mock import MagicMock, patch

from . import worker


def describe_worker_command():
    def it_is_a_click_command_named_worker():
        assert worker.worker.name == "worker"
        assert callable(worker.worker.callback)

    def it_runs_the_worker_over_stdin_and_stdout():
        fake_sys = MagicMock()
        with patch.object(worker, "configure") as configure:
            with patch.object(worker, "Worker") as worker_class:
                with patch.object(worker, "sys", fake_sys):
                    worker.worker.callback()
        configure.assert_called_once_with()
        worker_class.assert_called_once_with()
        worker_class.return_value.serve.assert_called_once_with(
            fake_sys.stdin, fake_sys.stdout
        )

    def it_configures_progress_before_serving():
        order = []
        with patch.object(
            worker, "configure", side_effect=lambda: order.append("configure")
        ):
            with patch.object(worker, "Worker") as worker_class:
                worker_class.return_value.serve.side_effect = (
                    lambda *args: order.append("serve")
                )
                with patch.object(worker, "sys", MagicMock()):
                    worker.worker.callback()
        assert order == ["configure", "serve"]
