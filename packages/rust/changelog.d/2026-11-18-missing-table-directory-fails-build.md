**Changed**

- **A table whose own directory does not exist now fails the build**, with `table '<name>': its directory <dir> does not exist`. A table's own directory is the one a config glob starting with `/`, `../` or `~/` names, or one passed to `Table::anchored`. Before, the build succeeded with no file watcher for any table, so later changes to every table went unseen. A missing subdirectory under an existing directory still builds. Every SDK and the CLI raise the error.
