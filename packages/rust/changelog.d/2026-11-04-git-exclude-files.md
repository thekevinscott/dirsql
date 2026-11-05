**Fixed**

- **Path-table scans honor `.git/info/exclude` and `core.excludesFile`.** Inside a git repo, files the repo's `info/exclude` or the global excludes file (by default `~/.config/git/ignore`) name are hidden like `.gitignore` matches, as git, fd and ripgrep hide them. Every `.gitignore` outranks both, and `--no-ignore` restores them.
