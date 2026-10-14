"""The committed speed page is the rendering of the committed suite report."""

import json
from pathlib import Path

from tests.e2e.bench_report import render_markdown

DOCS = Path(__file__).resolve().parents[5] / "docs"


def describe_speed_page():
    def it_is_the_rendering_of_the_committed_report():
        report = json.loads((DOCS / "speed.json").read_text())
        assert (DOCS / "speed.md").read_text() == render_markdown(report)

    def it_is_reachable_from_the_sidebar():
        config = (DOCS / ".vitepress" / "config.ts").read_text()
        assert "link: '/speed'" in config
