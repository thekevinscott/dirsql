**Changed**

- **A per-table command now starts parsing while the startup walk is still finding files.** A frontmatter table over 65,000 notes indexes about 5-15ms sooner on a loaded machine and about 15ms sooner on a restricted-CPU one. Output is unchanged.
