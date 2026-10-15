**Changed**

- **A config table's startup scan builds each file's relative path on the walk thread that found it, and inserts an `on-file` command's rows while the scanned files are still being grouped.** A config over many files, such as a notes vault with a frontmatter parser, starts faster. Query results are unchanged.
