"""Runs a test command several times, a fresh process each time, then judges what the tests recorded.

    measure.py --results FILE [--each GLOB] [--pairs 5] [--timeout 600] COMMAND

Each run, every case of a measured test appends one line to FILE: its own time and
memory, then the same for its baseline command. A test's time is the sum over its
cases. Its memory is the peak across them, counted from where the first case began.

With --each, COMMAND runs once per matching file with {file} replaced, for a runner
that would otherwise run every test file in one process.
"""

import argparse
import glob
import json
import os
import statistics
import subprocess
import sys
from collections import defaultdict
from pathlib import Path

parser = argparse.ArgumentParser()
parser.add_argument("--results", required=True, type=Path)
parser.add_argument("--each")
parser.add_argument("--pairs", type=int, default=5)
parser.add_argument("--timeout", type=int, default=600, help="seconds allowed for one run of the command")
parser.add_argument("command")
args = parser.parse_args()

results = args.results.resolve()
results.parent.mkdir(parents=True, exist_ok=True)
runs = defaultdict(list)
commands = [args.command]
if args.each:
    commands = [args.command.replace("{file}", file) for file in sorted(glob.glob(args.each, recursive=True))]
    if not commands:
        sys.exit(f"FAIL: no file matches {args.each}")
limits = {}

for _ in range(args.pairs):
    results.write_text("")
    for command in commands:
        try:
            status = subprocess.run(command, shell=True, env={**os.environ, "MEASURE": str(results)}, timeout=args.timeout)
        except subprocess.TimeoutExpired:
            sys.exit(f"FAIL: ran longer than {args.timeout} s: {command}")
        if status.returncode != 0:
            sys.exit(status.returncode)
    by_file = defaultdict(list)
    for line in map(json.loads, results.read_text().splitlines()):
        by_file[os.path.relpath(line["file"])].append(line)
        limits[os.path.relpath(line["file"])] = line["limits"]
    for file, lines in by_file.items():
        runs[file].append({
            "ms": sum(line["ms"] for line in lines), "base_ms": sum(line["base_ms"] for line in lines),
            "mb": max(line["mb"] for line in lines), "base_mb": max(line["base_mb"] for line in lines),
        })

if not runs:
    sys.exit("FAIL: no test recorded a measurement")

failed = False
report = []
for file, seen in sorted(runs.items()):
    print(file)
    for unit in ("ms", "mb"):
        got = [run[unit] for run in seen]
        base = statistics.mean(run[f"base_{unit}"] for run in seen)
        want = limits[file][unit]
        checks = {"max": max(got), "mean": statistics.mean(got), "times_baseline": statistics.mean(got) / base}
        over = [name for name, value in checks.items() if want.get(name) is not None and value > want[name]]
        failed = failed or bool(over)
        cells = "  ".join(f"{name} {value:.1f} (limit {want.get(name, 'none')})" for name, value in checks.items())
        print(f"  {unit}  {cells}  baseline {base:.1f}  {'FAIL: ' + ', '.join(over) if over else 'ok'}")
        report.append({"file": file, "unit": unit, "runs": got, "baseline": base, **checks, "limits": want, "over": over})

results.with_suffix(".report.json").write_text(json.dumps(report, indent=2) + "\n")
sys.exit(1 if failed else 0)
