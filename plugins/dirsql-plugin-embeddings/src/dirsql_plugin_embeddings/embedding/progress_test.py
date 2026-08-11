from unittest.mock import MagicMock, patch

from . import progress


def describe_stderr_is_tty():
    def it_reflects_sys_stderr_isatty_true():
        fake_sys = MagicMock()
        fake_sys.stderr.isatty.return_value = True
        with patch.object(progress, "sys", fake_sys):
            assert progress.stderr_is_tty() is True
        fake_sys.stderr.isatty.assert_called_once_with()

    def it_reflects_sys_stderr_isatty_false():
        fake_sys = MagicMock()
        fake_sys.stderr.isatty.return_value = False
        with patch.object(progress, "sys", fake_sys):
            assert progress.stderr_is_tty() is False


def describe_configure():
    def it_silences_hf_hub_progress_when_stderr_is_not_a_tty():
        with patch.object(progress, "stderr_is_tty", return_value=False):
            with patch.dict(progress.os.environ, clear=True):
                progress.configure()
                assert (
                    progress.os.environ["HF_HUB_DISABLE_PROGRESS_BARS"] == "1"
                )

    def it_keeps_an_explicit_user_setting():
        with patch.object(progress, "stderr_is_tty", return_value=False):
            with patch.dict(
                progress.os.environ,
                {"HF_HUB_DISABLE_PROGRESS_BARS": "0"},
                clear=True,
            ):
                progress.configure()
                assert (
                    progress.os.environ["HF_HUB_DISABLE_PROGRESS_BARS"] == "0"
                )

    def it_leaves_the_environment_alone_when_stderr_is_a_tty():
        with patch.object(progress, "stderr_is_tty", return_value=True):
            with patch.dict(progress.os.environ, clear=True):
                progress.configure()
                assert (
                    "HF_HUB_DISABLE_PROGRESS_BARS" not in progress.os.environ
                )
