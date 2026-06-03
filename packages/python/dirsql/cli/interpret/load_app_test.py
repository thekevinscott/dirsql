"""Unit tests for `load_app`."""

import importlib.util
import os
from pathlib import Path
from unittest.mock import patch

import pytest

from dirsql.cli.interpret import load_app as load_app_mod
from dirsql.cli.interpret.load_app import load_app


def describe_load_app():
    def it_returns_the_app_attribute_of_the_loaded_module(tmp_path: Path):
        config = tmp_path / "config.py"
        config.write_text("app = {'sentinel': 'value'}\n")
        result = load_app(str(config))
        assert result == {"sentinel": "value"}

    def it_resolves_relative_paths_against_the_cwd(tmp_path: Path):
        config = tmp_path / "config.py"
        config.write_text("app = 42\n")
        prev = os.getcwd()
        try:
            os.chdir(tmp_path)
            assert load_app("config.py") == 42
        finally:
            os.chdir(prev)

    def it_raises_import_error_when_the_file_does_not_exist(tmp_path: Path):
        missing = tmp_path / "nope.py"
        with pytest.raises((ImportError, FileNotFoundError)):
            load_app(str(missing))

    def it_raises_attribute_error_with_a_path_aware_message_when_app_is_missing(
        tmp_path: Path,
    ):
        config = tmp_path / "config.py"
        config.write_text("not_the_app = 1\n")
        with pytest.raises(AttributeError, match="must define a top-level `app"):
            load_app(str(config))

    def it_surfaces_exceptions_raised_during_module_exec(tmp_path: Path):
        config = tmp_path / "config.py"
        config.write_text("raise RuntimeError('synthetic boom')\n")
        with pytest.raises(RuntimeError, match="synthetic boom"):
            load_app(str(config))

    def it_raises_import_error_when_spec_from_file_location_returns_none(
        tmp_path: Path,
    ):
        with patch.object(
            load_app_mod.importlib.util,
            "spec_from_file_location",
            return_value=None,
        ):
            with pytest.raises(ImportError, match="could not load config"):
                load_app(str(tmp_path / "anything.py"))

    def it_raises_import_error_when_spec_has_no_loader(tmp_path: Path):
        class _SpecNoLoader:
            loader = None

        with patch.object(
            load_app_mod.importlib.util,
            "spec_from_file_location",
            return_value=_SpecNoLoader(),
        ):
            with pytest.raises(ImportError, match="could not load config"):
                load_app(str(tmp_path / "anything.py"))

    def it_absolutizes_the_passed_path_before_loading(tmp_path: Path):
        config = tmp_path / "config.py"
        config.write_text("app = 1\n")
        with patch.object(
            importlib.util,
            "spec_from_file_location",
            wraps=importlib.util.spec_from_file_location,
        ) as spec_call:
            load_app(str(config))
        # First positional arg after the synthetic module name is the
        # absolute path the loader was asked to read.
        called_path = spec_call.call_args.args[1]
        assert called_path == os.path.abspath(str(config))
