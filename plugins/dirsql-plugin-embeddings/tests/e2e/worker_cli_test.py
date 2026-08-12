"""E2E: the installed ``dirsql-plugin-embeddings worker`` CLI, nothing mocked.

Spawns the real console script (as installed on PATH) and speaks the wire
protocol over real pipes: real argparse dispatch, real model2vec inference,
real cachetta writes under a real ``XDG_CACHE_HOME``.

The model is the real on-disk model2vec model from conftest, passed through
the worker's ordinary model-override argument. The default
``minishlab/potion-retrieval-32M`` path is identical code minus the argument;
exercising it end-to-end needs a Hugging Face download, which sandboxed
environments block (run it manually where the network allows:
``echo '{"call": ["hello world"]}' | dirsql-plugin-embeddings worker``).

Run under an environment that has this plugin installed, e.g.:

    uv run python -m pytest tests/e2e -q
"""

import shutil


def worker_argv():
    script = shutil.which("dirsql-plugin-embeddings")
    assert script, "console script dirsql-plugin-embeddings must be on PATH"
    return [script, "worker"]


def describe_worker_cli():
    def it_serves_an_embed_request_over_real_pipes(spawn_worker, tiny_model):
        worker = spawn_worker(argv=worker_argv())
        assert worker.request("hello", tiny_model) == {"ok": [1.0, 0.0]}

    def it_survives_a_malformed_request_and_keeps_serving(
        spawn_worker, tiny_model
    ):
        worker = spawn_worker(argv=worker_argv())
        response = worker.send_line("{definitely not json")
        assert "err" in response
        assert worker.request("hello world", tiny_model) == {"ok": [0.5, 0.5]}

    def it_stays_silent_on_stderr_when_stderr_is_not_a_tty(
        spawn_worker, tiny_model
    ):
        worker = spawn_worker(argv=worker_argv())
        worker.request("hello", tiny_model)
        code, stderr = worker.close()
        assert code == 0
        assert stderr == ""

    def it_leaks_no_semaphore_when_torn_down_by_kill(spawn_worker, tiny_model):
        # dirsql's core tears the worker transport down with a kill, not a
        # graceful stdin close; nothing the worker allocated may depend on
        # interpreter shutdown to avoid a resource-tracker complaint.
        worker = spawn_worker(argv=worker_argv())
        worker.request("hello", tiny_model)
        worker.process.kill()
        worker.process.wait(timeout=30)
        stderr = worker.process.stderr.read()
        assert "leaked semaphore" not in stderr

    def it_errors_actionably_about_the_missing_glob_when_run_bare(spawn_worker):
        script = shutil.which("dirsql-plugin-embeddings")
        assert script
        worker = spawn_worker(argv=[script])
        code, stderr = worker.close()
        assert code == 2
        assert "Missing argument" in stderr
        assert "GLOB" in stderr

    def it_shows_group_help_on_help_flag(spawn_worker):
        script = shutil.which("dirsql-plugin-embeddings")
        assert script
        process = spawn_worker(argv=[script, "--help"]).process
        stdout, _ = process.communicate(timeout=30)
        assert process.returncode == 0
        assert "worker" in stdout
