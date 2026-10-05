**Fixed** Inside a git repo, a path-table scan applies the `.gitignore` files
above the directory it starts in, as git does: every one from the repo root
(the nearest directory holding `.git`) down. `'./docs/*.log'` now hides what a
root `*.log` rule ignores, as `'./**'` already did, and an index root inside a
repo honors the repo root's `.gitignore`. Naming a gitignored directory
(`'./dist'`) still scans it.
