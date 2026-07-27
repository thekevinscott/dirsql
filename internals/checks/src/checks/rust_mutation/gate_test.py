from unittest import mock

from checks.rust_mutation.gate import (
    CRATE_DIR,
    SURVIVOR_HINT,
    build_diff,
    run,
)


def _result(returncode=0, stdout=""):
    return mock.Mock(returncode=returncode, stdout=stdout)


def describe_build_diff():
    def diffs_the_merge_base_range_with_workspace_relative_paths():
        runner = mock.Mock(return_value=_result(stdout="DIFF"))
        assert build_diff("origin/main", runner) == "DIFF"
        runner.assert_called_once_with(
            ["git", "diff", "origin/main...HEAD"],
            capture_output=True,
            text=True,
            check=True,
        )

    def never_passes_relative_which_would_break_workspace_member_matching():
        runner = mock.Mock(return_value=_result())
        build_diff("abc123", runner)
        (argv,), _ = runner.call_args
        assert "--relative" not in argv


def describe_run():
    def writes_the_diff_and_drives_cargo_mutants_from_the_crate_dir():
        runner = mock.Mock(side_effect=[_result(stdout="DIFF"), _result(returncode=0)])
        writer = mock.Mock(return_value="/tmp/x.diff")
        assert run("origin/main", runner=runner, writer=writer) == 0
        writer.assert_called_once_with("DIFF")
        assert runner.call_args_list[1] == mock.call(
            ["cargo", "mutants", "--features", "cli", "--in-diff", "/tmp/x.diff"],
            cwd=CRATE_DIR,
        )

    def a_clean_run_prints_no_survivor_hint(capsys):
        runner = mock.Mock(side_effect=[_result(stdout="DIFF"), _result(returncode=0)])
        run("origin/main", runner=runner, writer=mock.Mock(return_value="/tmp/x.diff"))
        assert SURVIVOR_HINT not in capsys.readouterr().out

    def a_survivor_makes_the_gate_red_and_prints_the_fix_hint(capsys):
        runner = mock.Mock(side_effect=[_result(stdout="DIFF"), _result(returncode=2)])
        rc = run("origin/main", runner=runner, writer=mock.Mock(return_value="/tmp/x.diff"))
        assert rc == 2
        assert SURVIVOR_HINT in capsys.readouterr().out


def describe_write_temp_diff():
    def writes_content_to_a_named_diff_file_and_returns_its_path():
        from checks.rust_mutation.gate import _write_temp_diff

        handle = mock.MagicMock()
        handle.name = "/tmp/generated.diff"
        with mock.patch(
            "checks.rust_mutation.gate.tempfile.NamedTemporaryFile", return_value=handle
        ) as factory:
            assert _write_temp_diff("DIFF BODY") == "/tmp/generated.diff"
        factory.assert_called_once_with("w", suffix=".diff", delete=False)
        handle.write.assert_called_once_with("DIFF BODY")
        handle.close.assert_called_once_with()
