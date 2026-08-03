"""E2E test for `dirsql-checks wheel-extension-load` through the real CLI.

No mocking of any kind: spawns the packaged `dirsql-checks` console script as
a subprocess against a real dist directory. The full probe (install a built
wheel, load sqlite-vec) runs in CI's release-precheck `extension-load` job
against the precheck matrix's wheel; here the cheap no-wheel and
multiple-wheel contracts are exercised end to end.
"""

from __future__ import annotations

import shutil
import subprocess


def _cli() -> str:
    dirsql_checks = shutil.which("dirsql-checks")
    assert dirsql_checks is not None, (
        "`dirsql-checks` console script not on PATH -- run "
        "`uv run --project internals/checks pytest tests/e2e` "
        "or `uv sync --project internals/checks`"
    )
    return dirsql_checks


def describe_dirsql_checks_wheel_extension_load():
    def it_skips_cleanly_when_the_dist_dir_has_no_wheel(tmp_path):
        (tmp_path / "dirsql.tar.gz").write_text("")

        proc = subprocess.run(
            [_cli(), "wheel-extension-load", "--dist-dir", str(tmp_path)],
            capture_output=True,
            text=True,
        )

        assert proc.returncode == 0, proc.stderr
        assert "probe skipped" in proc.stdout

    def it_skips_cleanly_when_the_dist_dir_is_absent(tmp_path):
        proc = subprocess.run(
            [_cli(), "wheel-extension-load", "--dist-dir", str(tmp_path / "missing")],
            capture_output=True,
            text=True,
        )

        assert proc.returncode == 0, proc.stderr
        assert "probe skipped" in proc.stdout

    def it_fails_with_fix_instructions_on_multiple_wheels(tmp_path):
        (tmp_path / "a.whl").write_text("")
        (tmp_path / "b.whl").write_text("")

        proc = subprocess.run(
            [_cli(), "wheel-extension-load", "--dist-dir", str(tmp_path)],
            capture_output=True,
            text=True,
        )

        assert proc.returncode == 1
        assert "expected exactly one wheel" in proc.stderr
        assert "release-precheck.yml" in proc.stderr
