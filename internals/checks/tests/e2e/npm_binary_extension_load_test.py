"""E2E test for `dirsql-checks npm-binary-extension-load` through the real CLI.

No mocking of any kind: spawns the packaged `dirsql-checks` console script as
a subprocess against a real dist directory. The full probe (chmod the bundled
binary, load sqlite-vec through its `--extension` flag) runs in CI's
release-precheck `npm-extension-load` job against the precheck matrix's
artifact; here the cheap no-binary and multiple-binary contracts are
exercised end to end.
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


def describe_dirsql_checks_npm_binary_extension_load():
    def it_skips_cleanly_when_the_dist_dir_has_no_binary(tmp_path):
        (tmp_path / "README.md").write_text("")

        proc = subprocess.run(
            [_cli(), "npm-binary-extension-load", "--dist-dir", str(tmp_path)],
            capture_output=True,
            text=True,
        )

        assert proc.returncode == 0, proc.stderr
        assert "probe skipped" in proc.stdout

    def it_skips_cleanly_when_the_dist_dir_is_absent(tmp_path):
        proc = subprocess.run(
            [
                _cli(),
                "npm-binary-extension-load",
                "--dist-dir",
                str(tmp_path / "missing"),
            ],
            capture_output=True,
            text=True,
        )

        assert proc.returncode == 0, proc.stderr
        assert "probe skipped" in proc.stdout

    def it_fails_with_fix_instructions_on_multiple_binaries(tmp_path):
        (tmp_path / "a").mkdir()
        (tmp_path / "b").mkdir()
        (tmp_path / "a" / "dirsql").write_text("")
        (tmp_path / "b" / "dirsql").write_text("")

        proc = subprocess.run(
            [_cli(), "npm-binary-extension-load", "--dist-dir", str(tmp_path)],
            capture_output=True,
            text=True,
        )

        assert proc.returncode == 1
        assert "expected exactly one" in proc.stderr
        assert "release-precheck.yml" in proc.stderr
