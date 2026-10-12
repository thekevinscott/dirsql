"""Records, report and markdown for the performance suite (`just bench-suite`)."""

from __future__ import annotations

import json
import statistics
from pathlib import Path

RECORDS = "cases.jsonl"


def ratio_stats(ratios):
    low, high = min(ratios), max(ratios)
    median = statistics.median(ratios)
    return {"min": low, "median": median, "max": high, "spread": (high - low) / median}


def case_record(name, result, startup):
    natives = {}
    for native, seconds in result.native_seconds.items():
        net = [s - startup.native for s in seconds]
        natives[native] = {
            "native_median_s": statistics.median(seconds),
            "ratio": ratio_stats(
                [(d - startup.dirsql) / n for d, n in zip(result.dirsql_seconds, net)]
            ),
        }
    return {
        "name": name,
        "pairs": len(result.dirsql_seconds),
        "fastest": result.fastest,
        "ratio_vs_fastest": natives[result.fastest]["ratio"]["median"],
        "dirsql_median_s": statistics.median(result.dirsql_seconds),
        "startup": {"dirsql": startup.dirsql, "native": startup.native},
        "natives": natives,
        "skipped": list(result.skipped),
    }


def build_report(records, meta):
    return {**meta, "cases": sorted(records, key=lambda c: c["name"])}


def append_record(out, record):
    with (Path(out) / RECORDS).open("a") as file:
        file.write(json.dumps(record) + "\n")


def read_records(out):
    lines = (Path(out) / RECORDS).read_text().splitlines()
    return [json.loads(line) for line in lines]


def render_markdown(report):
    machine = ", ".join(f"{k}: {v}" for k, v in report["machine"].items())
    versions = ", ".join(f"{k}: {v}" for k, v in report["versions"].items())
    lines = [
        "# dirsql performance suite",
        "",
        f"- Commit: {report['commit']}",
        f"- Date: {report['date']}",
        f"- Machine: {machine}",
        f"- Versions: {versions}",
        (
            f"- Method: {report['pairs']} paired runs per case, interleaved; ratios"
            " are dirsql over the native after each side's startup, median across"
            " pairs"
        ),
        "",
        "| Case | Fastest native | dirsql / fastest | Spread | vs 1.0x |",
        "|---|---|---|---|---|",
    ]
    for case in report["cases"]:
        stats = case["natives"][case["fastest"]]["ratio"]
        verdict = "over" if case["ratio_vs_fastest"] > 1.0 else "ok"
        lines.append(
            f"| {case['name']} | {case['fastest']} | {case['ratio_vs_fastest']:.2f}x"
            f" | {stats['spread']:.0%} | {verdict} |"
        )
    for case in report["cases"]:
        lines += [
            "",
            f"## {case['name']}",
            "",
            f"dirsql median {case['dirsql_median_s']:.3f}s",
            "",
            "| Native | Median s | dirsql / native | Min | Max | Spread |",
            "|---|---|---|---|---|---|",
        ]
        for native, data in sorted(case["natives"].items()):
            r = data["ratio"]
            lines.append(
                f"| {native} | {data['native_median_s']:.3f} | {r['median']:.2f}x"
                f" | {r['min']:.2f}x | {r['max']:.2f}x | {r['spread']:.0%} |"
            )
        lines += [f"- {tool} (not installed)" for tool in case["skipped"]]
    return "\n".join(lines) + "\n"
