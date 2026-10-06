**Fixed**

- **On Linux, watch mode no longer puts an inotify watch on directories the scan skips.** A directory matched by an `ignore` pattern (such as `node_modules` or `.git`) and the reserved `.dirsql/` directory now cost no watches, so a large `node_modules` no longer exhausts `fs.inotify.max_user_watches`. A directory created inside watched territory is watched as it appears. macOS and Windows already cover a tree with one watch and are unchanged. (#1401)
