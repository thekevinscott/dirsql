"""Integration tests for the pytest-gate check against a real pytest subprocess.

Exercises `gate.run` with its default collaborators (`subprocess.run`,
`find_test_files`) -- a real pytest invocation over a real scratch directory,
never the packaged `dirsql-checks` CLI (that's the e2e tier).
"""

from __future__ import annotations

from checks.pytest_gate.gate import run


def describe_run_against_a_real_directory():
    def it_passes_when_the_real_suite_passes(tmp_path):
        (tmp_path / "sample_test.py").write_text("def test_ok():\n    assert True\n")

        assert run([str(tmp_path), "-q"]) == 0

    def it_fails_when_the_real_suite_fails(tmp_path):
        (tmp_path / "sample_test.py").write_text(
            "def test_broken():\n    assert False\n"
        )

        assert run([str(tmp_path), "-q"]) != 0

    def it_passes_when_the_directory_has_no_test_files(tmp_path):
        (tmp_path / "helper.py").write_text("x = 1\n")

        assert run([str(tmp_path), "-q"]) == 0
