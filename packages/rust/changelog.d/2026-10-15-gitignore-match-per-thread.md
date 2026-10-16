**Changed**

- **A path-table scan inside a git repo matches `.gitignore` rules without contention between walk threads.** Globs such as `./**` in a large repo answer faster. Which files are ignored is unchanged.
