"""Probe the version the release-built npm addon's CLI reports.

Loads the Release Precheck matrix's linux-x64 napi addon -- built by the same
`_matrix.yml` as the published one -- and runs its `runCli(["--version"])`. The
pipeline must stamp the planned release version into the crate whose
`CARGO_PKG_VERSION` the CLI prints. Every planned version is newer than the
literal committed in that crate's manifest, so an addon still reporting the
literal was built without the stamp.
"""

from __future__ import annotations

import os
import subprocess
from collections.abc import Callable

from checks.probe.probe_error import ProbeError
from checks.probe.require_zero import require_zero
from checks.wheel_extension_load.list_names import list_names

from .addon_names import addon_names
from .committed_version import committed_version

PRINT_VERSION = "process.exitCode = require(process.argv[1]).runCli(['--version'])"


def run(
    dist_dir: str,
    manifest: str,
    runner=subprocess.run,
    listdir=os.listdir,
    committed: Callable[[str], str] = committed_version,
) -> int:
    addons = addon_names(list_names(dist_dir, listdir))
    if len(addons) > 1:
        raise ProbeError(
            f"expected exactly one napi addon to probe, saw {addons}. "
            "Tighten the download-artifact pattern in release-ci.yml so only "
            "the linux-x64 addon lands in the probe's dist dir."
        )
    if not addons:
        print(
            f"No napi addon under {dist_dir} -- the precheck matrix planned no "
            "dirsql-npm build for this PR; version probe skipped."
        )
        return 0
    (addon_name,) = addons
    addon = os.path.abspath(os.path.join(dist_dir, addon_name))

    probe = runner(
        ["node", "-e", PRINT_VERSION, addon],
        stdin=subprocess.DEVNULL,
        capture_output=True,
        text=True,
    )
    require_zero(
        probe,
        f"`runCli(['--version'])` failed: stdout={probe.stdout!r}, stderr={probe.stderr!r}",
    )
    reported = probe.stdout.strip()
    literal = committed(manifest)
    if reported == f"dirsql {literal}":
        raise ProbeError(
            f"the npm addon's CLI reports `{reported}`, the version committed in "
            f"{manifest}: the release pipeline did not stamp the planned version "
            "into the crate the CLI reads its version from, so `npx dirsql "
            "--version` would misreport every release. putitoutthere stamps a "
            "napi build only when the crate's Cargo.toml sits at the npm "
            "package's `path` in putitoutthere.toml."
        )
    print(f"ok npm-addon-version: {addon_name} reports `{reported}`")
    return 0
