"""Entry point for `just bench-suite`.

Runs every speed-of-light case with every native timed in each paired,
interleaved run, then writes `report.json` and `report.md` beside the raw
per-case records. The cases are the e2e tests themselves, so the suite and the
ratchet never disagree on what a case is.

    python -m tests.e2e.bench_suite --pairs 100 --out /tmp/bench [-k expr]
"""

from __future__ import annotations

import argparse
import datetime
import json
import os
import platform
import shutil
import subprocess
import sys
from pathlib import Path

from .bench_report import build_report, read_records, render_markdown

HERE = Path(__file__).parent
TOOLS = (
    ("bash", ["bash", "--version"]),
    ("find", ["find", "--version"]),
    ("fd", ["fd", "--version"]),
    ("rg", ["rg", "--version"]),
    ("xargs", ["xargs", "--version"]),
    ("inotifywait", ["inotifywait", "--help"]),
)


def case_files(directory=HERE):
    return sorted(
        path
        for path in directory.glob("*_test.py")
        if "assert_speed_of_light" in path.read_text()
    )


def first_line(argv):
    if shutil.which(argv[0]) is None:
        return None
    proc = subprocess.run(argv, capture_output=True, text=True, check=False)
    lines = (proc.stdout or proc.stderr).splitlines()
    return lines[0].strip() if lines else None


def cpu_model():
    try:
        for line in Path("/proc/cpuinfo").read_text().splitlines():
            if line.startswith("model name"):
                return line.split(":", 1)[1].strip()
    except OSError:
        pass
    return platform.processor() or "unknown"


def git_commit():
    proc = subprocess.run(
        ["git", "rev-parse", "HEAD"], capture_output=True, text=True, check=False
    )
    return proc.stdout.strip() or "unknown"


def metadata(pairs):
    versions = {}
    for name, argv in TOOLS:
        versions[name] = first_line(argv)
    return {
        "commit": git_commit(),
        "date": datetime.datetime.now(datetime.timezone.utc).strftime(
            "%Y-%m-%dT%H:%M:%SZ"
        ),
        "machine": {
            "platform": platform.platform(),
            "cpu": cpu_model(),
            "cpus": os.cpu_count(),
        },
        "versions": {k: v for k, v in versions.items() if v},
        "pairs": pairs,
    }


def main(argv=None):
    parser = argparse.ArgumentParser()
    parser.add_argument("--pairs", type=int, default=100)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("-k", default="")
    args = parser.parse_args(argv)
    args.out.mkdir(parents=True, exist_ok=True)
    (args.out / "cases.jsonl").unlink(missing_ok=True)
    env = {
        **os.environ,
        "DIRSQL_BENCH_PAIRS": str(args.pairs),
        "DIRSQL_BENCH_OUT": str(args.out),
    }
    pytest = [sys.executable, "-m", "pytest", "-q", "-s", "-p", "no:cacheprovider"]
    if args.k:
        pytest += ["-k", args.k]
    code = subprocess.run(
        pytest + [str(p) for p in case_files()], env=env, check=False
    ).returncode
    report = build_report(read_records(args.out), metadata(args.pairs))
    (args.out / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    (args.out / "report.md").write_text(render_markdown(report))
    print(f"wrote {args.out}/report.json and {args.out}/report.md")
    return code


if __name__ == "__main__":
    sys.exit(main())
