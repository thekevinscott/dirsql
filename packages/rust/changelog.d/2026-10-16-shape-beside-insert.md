**Changed**

- **A per-table `on-file` hook's rows are shaped on their own thread while the previous chunk is inserted.** Startup over a large tree, such as a frontmatter table over 131k notes, finishes about 4% sooner. Query results are unchanged.
