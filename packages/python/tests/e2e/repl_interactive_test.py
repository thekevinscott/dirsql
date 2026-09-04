"""E2E tests for the REPL behaviors that need a terminal, not an output stream.

The `dirsql` REPL's line editor repaints by moving the cursor, so what a pipe
sees is a byte stream rather than a screen: a harness reading it can only guess
when a redraw has finished, and a keystroke sent into the gap after a statement
runs is read by nothing. These tests therefore drive the real console script
through [curtaincall](https://github.com/thekevinscott/curtaincall) -- a real
PTY behind a VT100 emulator -- and wait on the *screen* before each keystroke.

Nothing is mocked: real console script, real binary, real pty, real editor,
real filesystem, real SQLite. `HOME` and `XDG_DATA_HOME` point into the test's
own tmp tree, so the suite never touches the developer's history file.

What lives here is what only a screen can answer -- history recall with the up
arrow, history surviving the process, and `Ctrl+C` abandoning a line without
ending the session. The rest of the interactive contract (a statement ending
at its semicolon, the continuation prompt) is asserted from the byte stream by
`packages/rust/tests/repl_tty_e2e.rs`, which runs in CI.

Skipped below Python 3.12: curtaincall declares `requires-python >=3.12` while
this package's floor is 3.10 (thekevinscott/curtaincall#26).
"""

from __future__ import annotations

import os
import shutil

import pytest

pytest.importorskip(
    "curtaincall",
    minversion="0.3",
    reason="curtaincall requires Python >= 3.12; this package supports >= 3.10",
)

from curtaincall import expect

# curtaincall registers its `terminal` fixture through an entry point, so
# there is nothing to add to `pytest_plugins`.

_PROMPT = "dirsql>"


def _answer_cursor_queries(term) -> None:
    """Let the terminal answer the editor's cursor-position queries.

    The line editor asks where the cursor is (a `ESC [ 6 n` Device Status
    Report) and gives up when nothing answers, so without this every session
    dies with "the cursor position could not be read". pyte already knows the
    answer -- `report_device_status` formats the reply -- but its
    `write_process_input` hook is an inert stub, so the reply never reaches
    the process. Pointing that hook at the pty is the whole of the fix.
    """
    term._screen.write_process_input = term.write


def _cli() -> str:
    """Resolve the `dirsql` console script for this test env."""
    dirsql = shutil.which("dirsql")
    assert dirsql is not None, (
        "`dirsql` console script not on PATH -- run `uv run maturin develop`"
    )
    return dirsql


def describe_the_interactive_repl():
    @pytest.fixture
    def home(tmp_path):
        """A tmp HOME so history never lands in the developer's real one."""
        data = tmp_path / "data"
        data.mkdir()
        return data

    @pytest.fixture
    def open_repl(terminal, tmp_path, home):
        """Open a REPL over an empty directory and wait for its first prompt."""

        def _open():
            workdir = tmp_path / "tree"
            workdir.mkdir(exist_ok=True)
            # A tty gets a table; JSON keeps these cases on rows, not layout.
            term = terminal(
                f"sh -c 'cd {workdir} && exec {_cli()} --format json'",
                env={
                    **os.environ,
                    "XDG_DATA_HOME": str(home),
                    "TERM": "xterm",
                },
            )
            _answer_cursor_queries(term)
            expect(term.get_by_text(_PROMPT)).to_be_visible(timeout=30)
            return term

        return _open

    def it_recalls_the_previous_statement_with_the_up_arrow(open_repl):
        term = open_repl()
        term.submit("SELECT 41 AS n;")
        expect(term.get_by_text('[{"n":41}]')).to_be_visible(timeout=30)

        term.key_up()
        # The recalled text is on the line before it is submitted: without
        # this the arrow key and the enter could race the editor's redraw.
        expect(term.get_by_text("SELECT 41 AS n;")).to_be_visible(timeout=10)
        term.key_enter()

        expect(term.get_by_text('[{"n":41}]')).to_be_visible(timeout=30)
        term.submit("quit")
        expect(term).to_have_exited(timeout=30)
        assert term.exit_code == 0

    def it_keeps_history_across_sessions(open_repl):
        # The point of a history *file*: a query worked out yesterday is still
        # one up-arrow away today.
        first = open_repl()
        first.submit("SELECT 43 AS n;")
        expect(first.get_by_text('[{"n":43}]')).to_be_visible(timeout=30)
        first.key_ctrl_d()
        expect(first).to_have_exited(timeout=30)

        second = open_repl()
        second.key_up()

        expect(second.get_by_text("SELECT 43 AS n;")).to_be_visible(timeout=30)

    def it_writes_history_where_xdg_says(open_repl, home):
        term = open_repl()
        term.submit("SELECT 44 AS n;")
        expect(term.get_by_text('[{"n":44}]')).to_be_visible(timeout=30)
        term.key_ctrl_d()
        expect(term).to_have_exited(timeout=30)

        history = home / "dirsql" / "history"
        assert history.exists(), f"no history file at {history}"
        assert "SELECT 44 AS n;" in history.read_text()

    def it_abandons_a_line_on_ctrl_c_without_ending_the_session(open_repl):
        # #988's reason for routing the interrupt through the editor rather
        # than a signal handler: the process must survive it.
        term = open_repl()
        term.write("SELECT 45 AS n")
        expect(term.get_by_text("SELECT 45 AS n")).to_be_visible(timeout=10)

        term.key_ctrl_c()

        term.submit("SELECT 46 AS n;")
        expect(term.get_by_text('[{"n":46}]')).to_be_visible(timeout=30)
        assert term.is_alive, "Ctrl+C must not kill the session"
        expect(term.get_by_text('[{"n":45}]')).not_to_be_visible(timeout=5)

        term.submit("quit")
        expect(term).to_have_exited(timeout=30)
        assert term.exit_code == 0

    def it_shows_a_continuation_prompt_until_the_terminator(open_repl):
        term = open_repl()
        term.submit("SELECT")

        expect(term.get_by_text("...>")).to_be_visible(timeout=10)

        term.submit("47 AS n;")
        expect(term.get_by_text('[{"n":47}]')).to_be_visible(timeout=30)
        term.key_ctrl_d()
        expect(term).to_have_exited(timeout=30)
