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


def keep_sigint_fatal(handler=signal.signal):
    """Give SIGINT its default disposition for the core's run, returning the prior one.

    No Python-level handler can end a run in progress: `run_cli` detaches the
    GIL for its whole duration, so nothing reaches the eval loop until the
    core has already finished — CPython's `default_int_handler` included. Only
    a disposition the kernel acts on by itself works, which is `SIG_DFL`, and
    it is what the standalone binary runs with.

    `dirsql server` keeps its exit code: signal-hook (which tokio uses)
    replaces the disposition when the server registers its handlers, and does
    not re-raise `SIG_DFL` afterwards, so the core's graceful shutdown is the
    only thing that acts and its 0 survives.
    """
    return handler(signal.SIGINT, signal.SIG_DFL)


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

    previous = keep_sigint_fatal()
    try:
        return run_in_process(argv=argv, module="dirsql._dirsql")
    except Exception as exc:
        print(f"dirsql: {exc}", file=sys.stderr)
        return 1
    finally:
        signal.signal(signal.SIGINT, previous)
