"""E2E test for `dirsql-checks npm-addon-version` through the real CLI.

No mocking of any kind: spawns the packaged `dirsql-checks` console script as
a subprocess against a real dist directory. The full probe (load a built
addon, run its CLI) runs in CI's release-ci `npm-addon-version` job against
the precheck matrix's addon; here the cheap no-addon and multiple-addon
contracts are exercised end to end.
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


def describe_dirsql_checks_npm_addon_version():
    def it_skips_cleanly_when_the_dist_dir_has_no_addon(tmp_path):
        (tmp_path / "package.json").write_text("{}")

        proc = subprocess.run(
            [_cli(), "npm-addon-version", "--dist-dir", str(tmp_path)],
            capture_output=True,
            text=True,
        )

        assert proc.returncode == 0, proc.stderr
        assert "version probe skipped" in proc.stdout

    def it_fails_with_fix_instructions_on_multiple_addons(tmp_path):
        (tmp_path / "a.node").write_text("")
        (tmp_path / "b.node").write_text("")

        proc = subprocess.run(
            [_cli(), "npm-addon-version", "--dist-dir", str(tmp_path)],
            capture_output=True,
            text=True,
        )

        assert proc.returncode == 1
        assert "expected exactly one napi addon" in proc.stderr
        assert "release-ci.yml" in proc.stderr
