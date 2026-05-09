#!/usr/bin/env python3
"""Stage the `dirsql` CLI binary into the Python package data dir so
`maturin build` ships it inside the wheel.

Run this BEFORE `maturin build`. `[tool.maturin].include` in
`packages/python/pyproject.toml` picks up `python/dirsql/_binary/*` and
`[project.scripts] dirsql = "dirsql._cli.main:main"` makes the launcher
exec the bundled binary.

By default builds the host triple. Pass `--target <triple>` to
cross-compile (the rust toolchain must already have that target
installed: `rustup target add <triple>`).
"""

from __future__ import annotations

import argparse
import shutil
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
RUST_MANIFEST = REPO / "packages" / "rust" / "Cargo.toml"
STAGE_DIR = REPO / "packages" / "python" / "python" / "dirsql" / "_binary"


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument(
        "--target",
        default=None,
        help="Cargo target triple (e.g. x86_64-unknown-linux-gnu). "
        "Default: host triple.",
    )
    p.add_argument(
        "--exe",
        action="store_true",
        help="Source/dest binary name is `dirsql.exe` (Windows targets).",
    )
    args = p.parse_args()

    cargo_cmd = [
        "cargo",
        "build",
        "--release",
        "--bin",
        "dirsql",
        "--features",
        "cli",
        "--manifest-path",
        str(RUST_MANIFEST),
    ]
    if args.target:
        cargo_cmd += ["--target", args.target]

    print(f"+ {' '.join(cargo_cmd)}", file=sys.stderr)
    result = subprocess.run(cargo_cmd)
    if result.returncode != 0:
        return result.returncode

    bin_name = "dirsql.exe" if args.exe else "dirsql"
    target_dir = REPO / "target"
    if args.target:
        src = target_dir / args.target / "release" / bin_name
    else:
        src = target_dir / "release" / bin_name

    if not src.is_file():
        print(f"stage_cli_binary: missing {src}", file=sys.stderr)
        return 1

    STAGE_DIR.mkdir(parents=True, exist_ok=True)
    dst = STAGE_DIR / bin_name
    shutil.copyfile(src, dst)
    dst.chmod(0o755)
    print(f"staged {src} -> {dst}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
