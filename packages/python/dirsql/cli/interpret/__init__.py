"""`dirsql interpret <config>` -- long-running native config helper (#196).

Loads a Python config file, takes its module-level ``app = DirSQL(...)``,
and serves ``extract`` requests over NDJSON on stdin/stdout. One line in,
one line out, sequential.

See the individual submodules for behavior:
- ``run`` -- subcommand entry point invoked from ``dirsql.cli.main``
- ``load_app`` -- importlib-based config loader
- ``write_message`` -- single-line NDJSON writer
- ``dispatch_extract`` -- per-request handler

No top-level re-exports: the function is at ``.run.run``. Re-exporting
``run`` at the package level would shadow the submodule (so
``from . import run`` in tests would resolve to the function and break
``patch.object(run_module, "load_app", ...)``).
"""
