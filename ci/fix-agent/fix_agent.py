# /// script
# requires-python = ">=3.12"
# dependencies = ["botbox"]
# ///
"""One tick of the fixing agent. Reads the state, acts on what it finds, exits.

Needs `gh` and `git` logged in as the bot, and Docker. The box gets no GitHub
access: this script does the push, the pull request, the merge and the
confirming run.
"""

import argparse
import asyncio
import json
import subprocess
import tempfile
from pathlib import Path

from botbox import Botbox, ClaudeAgent

HERE = Path(__file__).parent
WORKFLOW = "markdown-tests.yml"
REGISTRIES = [
    "crates.io", "index.crates.io", "static.crates.io",
    "pypi.org", "files.pythonhosted.org",
    "registry.npmjs.org",
]
OWNER_ONLY = ("tests/adhoc/", "tests/fixtures/")


def sh(*args: str, cwd: Path | None = None, check: bool = True) -> str:
    done = subprocess.run(args, cwd=cwd, check=check, capture_output=True, text=True)
    return done.stdout.strip()


def read_state(repo: str) -> dict:
    done = subprocess.run(
        ["gh", "api", f"repos/{repo}/contents/state.json?ref=ci-state",
         "-H", "Accept: application/vnd.github.raw"],
        capture_output=True, text=True,
    )
    return json.loads(done.stdout) if done.returncode == 0 else {}


def write_state(repo: str, change: dict, message: str) -> None:
    with tempfile.TemporaryDirectory() as tmp:
        sh("git", "clone", "-q", "--branch", "ci-state", "--depth", "1",
           f"git@github.com:{repo}.git", tmp)
        path = Path(tmp) / "state.json"
        path.write_text(json.dumps(json.loads(path.read_text()) | change, indent=2) + "\n")
        sh("git", "commit", "-qm", message, "state.json", cwd=Path(tmp))
        sh("git", "push", "-q", "origin", "ci-state", cwd=Path(tmp))


def run_in_progress(repo: str) -> bool:
    runs = json.loads(sh("gh", "run", "list", "--repo", repo, "--workflow", WORKFLOW,
                         "--limit", "5", "--json", "status"))
    return any(run["status"] != "completed" for run in runs)


def ask_owner(repo: str, clone: Path, sha: str, question: str, owner: str) -> None:
    branch = f"ci-question/{sha[:8]}"
    sh("git", "checkout", "-q", "-b", branch, cwd=clone)
    sh("git", "add", "-A", cwd=clone)
    sh("git", "commit", "-q", "--allow-empty", "-m", "Work in progress on a red main", cwd=clone)
    sh("git", "push", "-q", "-f", "origin", branch, cwd=clone)
    url = sh("gh", "pr", "create", "--repo", repo, "--base", "main", "--head", branch, "--draft",
             "--title", f"Main is red at {sha[:8]}: the fixing agent needs you",
             "--body", question, "--reviewer", owner, cwd=clone)
    write_state(repo, {"waiting": True, "question": url}, f"Waiting on the owner: {url}")
    print(f"Asked the owner: {url}")


async def tick(repo: str, owner: str, runs: Path, give_up_after: int) -> None:
    state = read_state(repo)
    if not state.get("red"):
        return print("Not red.")
    if state.get("waiting"):
        return print(f"Waiting on the owner: {state.get('question')}")
    if state.get("fails", 0) >= give_up_after:
        return print("Out of tries; the workflow rolls back.")
    if run_in_progress(repo):
        return print("A run is in progress.")

    sha, kind = state["red_sha"], state.get("red_kind", "plain")
    work = Path(tempfile.mkdtemp(prefix="dirsql-fix-"))
    clone, output = work / "repo", runs / f"{sha[:8]}-{state.get('fails', 0)}"
    sh("git", "clone", "-q", f"git@github.com:{repo}.git", str(clone))

    # The box's package installs leave a store in the mount.
    with (clone / ".git/info/exclude").open("a") as exclude:
        exclude.write(".pnpm-store/\n")

    log = clone / ".fix-agent-run.log"
    question = clone / ".fix-agent-question.md"
    failed = json.loads(sh("gh", "run", "list", "--repo", repo, "--workflow", WORKFLOW,
                           "--status", "failure", "--limit", "1", "--json", "databaseId"))
    if failed:
        log.write_text(sh("gh", "run", "view", str(failed[0]["databaseId"]), "--repo", repo,
                          "--log-failed", check=False))

    prompt = (HERE / "prompt.md").read_text().format(
        kind=kind, sha=sha, recipe="measure-md" if kind == "measured" else "test-md",
        log=f"/repo/{log.name}", question=f"/repo/{question.name}",
    )
    agent = ClaudeAgent()
    box = Botbox(agent, dockerfile=HERE / "Dockerfile", allow=REGISTRIES, timeout=3 * 60 * 60)
    await box.run(agent.command(prompt), output=output, mounts=[(clone, "/repo", "rw")], workdir="/repo")

    asked = question.read_text() if question.exists() else None
    log.unlink(missing_ok=True)
    question.unlink(missing_ok=True)
    changed = sh("git", "status", "--porcelain", cwd=clone).splitlines()
    touched_tests = [line for line in changed if line[3:].startswith(OWNER_ONLY)]

    if asked or touched_tests or not changed:
        reason = asked or (
            "The agent changed tests, which only the owner may do:\n\n" + "\n".join(touched_tests)
            if touched_tests else "The agent made no change and wrote no question."
        )
        return ask_owner(repo, clone, sha, f"{reason}\n\nTranscripts on duncan: `{output}`", owner)

    branch = f"ci-fix/{sha[:8]}-{state.get('fails', 0)}"
    sh("git", "checkout", "-q", "-b", branch, cwd=clone)
    sh("git", "add", "-A", cwd=clone)
    sh("git", "commit", "-qm", f"Fix the tests that failed at {sha[:8]}", cwd=clone)
    sh("git", "push", "-q", "origin", branch, cwd=clone)
    url = sh("gh", "pr", "create", "--repo", repo, "--base", "main", "--head", branch,
             "--title", f"Fix the tests that failed at {sha[:8]}",
             "--body", f"Opened by the fixing agent while main is red.\n\nTranscripts on duncan: `{output}`",
             cwd=clone)
    sh("gh", "pr", "checks", url, "--watch", "--required", "--fail-fast", cwd=clone)
    sh("gh", "pr", "merge", url, "--squash", "--delete-branch", cwd=clone)
    sh("gh", "workflow", "run", WORKFLOW, "--repo", repo, "--ref", "main")
    print(f"Merged {url} and started a confirming run.")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--repo", default="thekevinscott/dirsql")
    parser.add_argument("--owner", default="thekevinscott")
    parser.add_argument("--runs", type=Path, default=Path.home() / ".local/state/dirsql-fix-agent")
    parser.add_argument("--give-up-after", type=int, default=3)
    args = parser.parse_args()
    asyncio.run(tick(args.repo, args.owner, args.runs, args.give_up_after))


if __name__ == "__main__":
    main()
