"""importlib-based config loader for the `dirsql interpret` helper.

The user's config is a regular Python module that constructs a
``DirSQL`` instance and assigns it to a top-level ``app`` name. We
import it via ``importlib.util.spec_from_file_location`` so the user
doesn't have to put their config on ``sys.path``.
"""

from __future__ import annotations

import importlib.util
import os
from typing import Any


def load_app(config_path: str) -> Any:
    """Import ``config_path`` and return its ``app`` attribute.

    Raises ``ImportError`` if the file can't be loaded as a module, and
    ``AttributeError`` if the module loads but defines no ``app``.
    """
    abs_path = os.path.abspath(config_path)
    spec = importlib.util.spec_from_file_location("_dirsql_user_config", abs_path)
    if spec is None or spec.loader is None:
        raise ImportError(f"could not load config: {config_path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    if not hasattr(module, "app"):
        raise AttributeError(
            f"{config_path}: module must define a top-level `app = DirSQL(...)`"
        )
    return module.app
