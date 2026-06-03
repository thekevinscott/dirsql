"""`dirsql interpret <config>` -- long-running native config helper (#196).

Loads a Python config file, takes its module-level ``app = DirSQL(...)``,
and serves ``extract`` requests over NDJSON on stdin/stdout. One line in,
one line out, sequential.

See the individual submodules for behavior:
- ``run`` -- subcommand entry point invoked from ``dirsql.cli.main``
- ``load_app`` -- importlib-based config loader
- ``write_message`` -- single-line NDJSON writer
- ``dispatch_extract`` -- per-request handler
"""

from .run import run

__all__ = ["run"]
