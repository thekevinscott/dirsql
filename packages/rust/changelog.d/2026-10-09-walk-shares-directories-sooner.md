**Changed**

- **A tree walk now shares its directories across cores after eight directories (was 128), and judges a directory's entries across cores only from 65536 entries (was 4096).** Globs over a few large directories, such as `./*/*.md`, list faster. Output is unchanged.
