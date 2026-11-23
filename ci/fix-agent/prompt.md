The hourly tests on the main branch of this repository failed. Get them passing.

The failing run was a {kind} run on commit {sha}. Run it yourself to see the failure:

    just {recipe}-python
    just {recipe}-typescript
    just {recipe}-rust

The log of the failing run on GitHub is in `{log}`.

Rules:

- Fix the code, not the tests. Do not edit anything under `tests/adhoc/` or `tests/fixtures/`, and do not change a limit to make a measured run pass.
- Do not commit. Leave your changes in the working tree.
- You have no access to GitHub. Do not try to push or open a pull request.
- If the only fix is to change a test, or you need a decision from the owner, do not guess. Write the question to `{question}`, with what you found and the options, and stop.
- When the three commands above pass, stop.
