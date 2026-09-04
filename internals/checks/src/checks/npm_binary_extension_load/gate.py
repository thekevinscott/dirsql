"""Functional probe for the npm bundled-cli binary: load a real SQLite
extension through the precheck artifact's compiled `dirsql` (#762).

The Release Precheck matrix builds the npm bundled-cli Linux binary with
the same pipeline as the published `@dirsql/cli-linux-x64-gnu` package;
this probe downloads that artifact, pip-installs `sqlite-vec` into a
fresh venv for its loadable `vec0` library, and runs
`dirsql query "SELECT vec_version() AS v" -c <cfg>` against the binary
directly, with the config declaring that library's literal path. A
statically-linked binary cannot `dlopen`, so every extension load fails
with `Dynamic loading not supported` -- the published-0.4.x regression
this gate pins (#762), invisible to every other tier, which runs a
locally cargo-built (dynamic) binary.

The extension goes through a `-c` config rather than the `--extension`
flag the npm launcher uses because the binary drops `--extension`
entirely when no `-c` accompanies it (#772); either way the dlopen this
probe exists to exercise is the same one.

Effects funnel through injected callables (runner/walker/mkdtemp/...) so
every stage is unit-testable without spawning a build.
"""

from __future__ import annotations

import os
import subprocess
import sys
import tempfile

from checks.wheel_extension_load.gate import (
    ProbeError,
    _require_zero,
    bin_subdir,
    write_text,
)

from .diagnose import diagnose

PROBE_SQL = "SELECT vec_version() AS v"
ENTRYPOINT = "sqlite3_vec_init"
BIN_NAME = "dirsql"


def config_for(library_path: str) -> str:
    return (
        f'[[dirsql.extension]]\npath = "{library_path}"\n'
        f'entrypoint = "{ENTRYPOINT}"\n'
    )


def find_binaries(dist_dir: str, walker=os.walk) -> list[str]:
    return sorted(
        os.path.join(parent, name)
        for parent, _dirs, names in walker(dist_dir)
        for name in names
        if name == BIN_NAME
    )


def run(
    dist_dir: str,
    runner=subprocess.run,
    walker=os.walk,
    mkdtemp=tempfile.mkdtemp,
    makedirs=os.makedirs,
    chmod=os.chmod,
    writer=write_text,
    abspath=os.path.abspath,
) -> int:
    binaries = find_binaries(dist_dir, walker)
    if len(binaries) > 1:
        raise ProbeError(
            f"expected exactly one `{BIN_NAME}` binary to probe, saw "
            f"{binaries}. Tighten the download-artifact pattern in "
            "release-precheck.yml so only the linux-x64-gnu bundled-cli "
            "artifact lands in the probe's dist dir."
        )
    if not binaries:
        print(
            f"No `{BIN_NAME}` binary under {dist_dir} -- the precheck matrix "
            "planned no npm bundled-cli build for this PR; extension-load "
            "probe skipped."
        )
        return 0
    # The probe runs the binary with `cwd` set to a scratch dir, so a relative
    # `--dist-dir` (what release-precheck.yml passes) would otherwise resolve
    # against that scratch dir at exec time and die with ENOENT.
    (binary,) = binaries
    binary = abspath(binary)
    # actions/download-artifact does not preserve the executable bit.
    chmod(binary, 0o755)

    staging = mkdtemp("dirsql-npm-extension-probe-")
    venv_dir = os.path.join(staging, "venv")
    made = runner(
        [sys.executable, "-m", "venv", venv_dir],
        capture_output=True,
        text=True,
    )
    _require_zero(made, f"venv creation failed:\n{made.stderr}")
    venv_bin = os.path.join(venv_dir, bin_subdir())

    install = runner(
        [os.path.join(venv_bin, "pip"), "install", "--no-input", "sqlite-vec"],
        capture_output=True,
        text=True,
    )
    _require_zero(install, f"pip install failed:\n{install.stderr}")

    located = runner(
        [
            os.path.join(venv_bin, "python"),
            "-c",
            "import sqlite_vec; print(sqlite_vec.loadable_path())",
        ],
        capture_output=True,
        text=True,
    )
    _require_zero(located, f"locating sqlite-vec's loadable library failed:\n{located.stderr}")
    vec_path = located.stdout.strip()

    scratch = os.path.join(staging, "data")
    makedirs(scratch)
    config = os.path.join(staging, "ext.toml")
    writer(config, config_for(vec_path))

    probe = runner(
        [
            binary,
            "query",
            PROBE_SQL,
            "--config",
            config,
        ],
        cwd=scratch,
        stdin=subprocess.DEVNULL,
        capture_output=True,
        text=True,
    )
    if probe.returncode != 0:
        raise ProbeError(diagnose(probe))
    if '"v"' not in probe.stdout:
        raise ProbeError(
            f"probe query returned no row: "
            f"stdout={probe.stdout!r}\nstderr={probe.stderr!r}"
        )
    print(
        f"ok npm-binary-extension-load: {binary} loaded sqlite-vec "
        f"({probe.stdout.strip()})"
    )
    return 0
