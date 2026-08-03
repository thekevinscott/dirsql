"""Functional probe for the wheel's bundled binary: load a real SQLite
extension through the installed CLI (#755).

Installs the release-shape wheel (the Release Precheck matrix artifact,
built by the same pipeline as the published wheel) plus `sqlite-vec` into
a fresh venv, then runs `dirsql query "SELECT vec_version() AS v"` with a
config declaring the extension. A statically-linked bundled binary cannot
`dlopen`, so every extension load fails with `Dynamic loading not
supported` -- a defect invisible to every other tier, which runs a locally
cargo-built (dynamic) binary.

Effects funnel through injected callables (runner/listdir/mkdtemp/...) so
every stage is unit-testable without spawning a build.
"""

from __future__ import annotations

import os
import subprocess
import sys
import tempfile

PROBE_SQL = "SELECT vec_version() AS v"
CONFIG = '[[dirsql.extension]]\npath = "sqlite_vec"\nentrypoint = "sqlite3_vec_init"\n'
STATIC_MARKER = "Dynamic loading not supported"


class ProbeError(RuntimeError):
    """A probe stage failed -- carries a human-readable diagnostic."""


def _require_zero(result, message: str) -> None:
    if result.returncode != 0:
        raise ProbeError(message)


def bin_subdir(os_name: str = os.name) -> str:
    return {"nt": "Scripts"}.get(os_name, "bin")


def list_names(dist_dir: str, listdir=os.listdir) -> list[str]:
    try:
        return list(listdir(dist_dir))
    except FileNotFoundError:
        return []


def sole_wheel(names) -> str | None:
    wheels = sorted(name for name in names if name.endswith(".whl"))
    if not wheels:
        return None
    if len(wheels) > 1:
        raise ProbeError(
            f"expected exactly one wheel to probe, saw {wheels}. "
            "Tighten the download-artifact pattern in release-precheck.yml "
            "so only the x86_64 Linux wheel lands in the probe's dist dir."
        )
    return wheels[0]


def write_text(path: str, content: str) -> None:
    with open(path, "w", encoding="utf-8") as handle:
        handle.write(content)


def diagnose(result) -> str:
    output = f"stdout={result.stdout!r}\nstderr={result.stderr!r}"
    if STATIC_MARKER in result.stderr:
        return (
            f"the bundled `dirsql` binary in this wheel cannot load SQLite "
            f"extensions ({STATIC_MARKER!r}): it is statically linked "
            "(static-pie), and a static binary cannot dlopen. The release "
            "pipeline (putitoutthere `_matrix.yml`, #381) compiles the pypi "
            "bundle_cli binary against the musl-mapped triple; the fix is to "
            "build the wheel's bundled binary against the declared "
            "*-linux-gnu triple (the wheel's manylinux tag already gates the "
            "glibc floor), not to change this probe. See dirsql#755.\n"
            f"{output}"
        )
    return f"`dirsql query` against the installed wheel failed.\n{output}"


def run(
    dist_dir: str,
    *,
    runner=subprocess.run,
    listdir=os.listdir,
    mkdtemp=tempfile.mkdtemp,
    makedirs=os.makedirs,
    writer=write_text,
) -> int:
    wheel_name = sole_wheel(list_names(dist_dir, listdir))
    if wheel_name is None:
        print(
            f"No wheel under {dist_dir} -- the precheck matrix planned no "
            "dirsql-py build for this PR; extension-load probe skipped."
        )
        return 0
    wheel = os.path.join(dist_dir, wheel_name)

    staging = mkdtemp("dirsql-extension-probe-")
    venv_dir = os.path.join(staging, "venv")
    made = runner(
        [sys.executable, "-m", "venv", venv_dir],
        capture_output=True,
        text=True,
    )
    _require_zero(made, f"venv creation failed:\n{made.stderr}")
    venv_bin = os.path.join(venv_dir, bin_subdir())

    install = runner(
        [os.path.join(venv_bin, "pip"), "install", "--no-input", wheel, "sqlite-vec"],
        capture_output=True,
        text=True,
    )
    _require_zero(install, f"pip install failed:\n{install.stderr}")

    scratch = os.path.join(staging, "data")
    makedirs(scratch)
    config = os.path.join(staging, "ext.toml")
    writer(config, CONFIG)

    probe = runner(
        [
            os.path.join(venv_bin, "dirsql"),
            "query",
            PROBE_SQL,
            "--config",
            config,
            "--no-plugin",
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
    print(f"ok extension-load: {wheel_name} loaded sqlite-vec ({probe.stdout.strip()})")
    return 0
