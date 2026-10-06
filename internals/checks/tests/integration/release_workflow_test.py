"""The real release.yml must hand the PyPI tag job the versions it uploaded.

Without `expect`, `reconcile` reads PyPI's cached project pointer seconds after
the upload, finds nothing, and cuts no tag; the next run then re-plans the same
version and tags it at a later commit.
"""

from __future__ import annotations

from pathlib import Path

import yaml

WORKFLOW = Path(__file__).resolve().parents[4] / ".github" / "workflows" / "release.yml"


def pypi_tag_job() -> dict:
    return yaml.safe_load(WORKFLOW.read_text(encoding="utf-8"))["jobs"]["pypi-tag"]


def test_pypi_tag_expects_the_delegated_packages():
    job = pypi_tag_job()

    assert job["with"]["expect"] == "${{ needs.release.outputs.delegated_packages }}"


def test_pypi_tag_reads_the_release_outputs():
    job = pypi_tag_job()

    assert set(job["needs"]) == {"release", "pypi-publish"}


def test_pypi_tag_runs_only_after_a_successful_upload():
    job = pypi_tag_job()

    assert "needs.pypi-publish.result == 'success'" in job["if"]
