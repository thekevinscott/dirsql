**Changed** A `.gitignore` applies to path-table scans only inside a git
repo, as in git, fd and ripgrep: one is in force only when a directory
holding `.git` encloses it. Outside a repo none applies, at the scan's start,
above it or below it, so a `~/.claude` whose `.gitignore` is `*` lists its
files. Inside a repo nothing changes. `--no-ignore` is unchanged.
