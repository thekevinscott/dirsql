"""Console-script entry point. Runs the CLI in-process through the compiled
extension module — the same `_dirsql` the SDK imports — so the wheel ships one
copy of the core instead of a `.so` plus a bundled binary (#738).

All argv is forwarded transparently to the core, which owns subcommand
dispatch; the launcher only prepends what the core cannot work out for itself
(plugin config fragments, resolved extension paths).
"""

from __future__ import annotations

import signal
import sys

from bin_shim import main as run_in_process

from .discover_plugins.with_discovered_plugins import with_discovered_plugins
from .resolve_config_extensions import with_resolved_extensions


def _absorb_interrupt(*_args: object) -> None:
    """Let the core's own shutdown decide the exit code on SIGINT.

    signal-hook (which tokio uses) *chains*: it runs tokio's handler — which
    drives `dirsql server`'s graceful shutdown, after which `run_cli` returns
    0 — and then whatever handler was installed before it. CPython's default
    is `default_int_handler`, which raises `KeyboardInterrupt`; that lands
    after `run_cli` has already returned 0 and turns a clean shutdown into a
    130. This handler occupies that slot without raising, so the core's exit
    code is the one that survives, exactly as it does when the CLI is its own
    process.

    A signal arriving when the core is NOT handling signals still terminates:
    `run_cli` is only reached with this installed, and it returns promptly for
    every non-server command.
    """


def with_core_owned_signals(handler=signal.signal):
    """Install `_absorb_interrupt` for SIGINT and return the prior handler."""
    return handler(signal.SIGINT, _absorb_interrupt)


def main(argv: list[str] | None = None) -> int:
    if argv is None:
        argv = sys.argv[1:]

    # Discover installed plugins (CLI only) and inject their config fragments as
    # `-c` flags before resolving extensions; then resolve any package-name
    # extensions in a TOML config here (the core can't) as `--extension`
    # flags. Both are no-ops when nothing applies.
    try:
        argv = with_discovered_plugins(argv)
        argv = with_resolved_extensions(argv)
    except Exception as exc:
        print(f"dirsql: {exc}", file=sys.stderr)
        return 1

    previous = with_core_owned_signals()
    try:
        return run_in_process(argv=argv, module="dirsql._dirsql")
    except Exception as exc:
        print(f"dirsql: {exc}", file=sys.stderr)
        return 1
    finally:
        signal.signal(signal.SIGINT, previous)
