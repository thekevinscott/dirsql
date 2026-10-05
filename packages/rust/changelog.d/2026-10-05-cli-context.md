**Added**

- **`dirsql context` prints a usage guide for agents.** The first line is `dirsql <version>`, then markdown sections on usage (path-table syntax, columns, what a scan skips), recipes, and fixes for common errors. The guide is compiled into the binary, so it always describes the version that prints it. It takes no flags and exits 0. (#1264)
