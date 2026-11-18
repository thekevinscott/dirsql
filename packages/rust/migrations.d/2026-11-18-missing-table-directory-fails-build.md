### A table whose own directory is missing fails the build

#### Summary

Building a `DirSQL` (Rust, Python, TypeScript) or running the CLI with a table whose own directory does not exist now fails with `table '<name>': its directory <dir> does not exist`. A table's own directory is the one a config glob starting with `/`, `../` or `~/` names, or one passed to `Table::anchored`. Builds that used to succeed with such a table now fail. A missing subdirectory under an existing directory, such as `data/*.json` with no `data/`, is unaffected.

#### Required changes

| Before | After |
| --- | --- |
| A config with `glob = "../incoming/*.json"` and no `../incoming` built, and nothing it held was ever watched | Create the directory before building, or remove the table until it exists |

#### Deprecations removed

_None._

#### Behavior changes without code changes

The build raises instead of succeeding. Before, the build also started no file watcher for any table, so later changes to every table went unseen until something called `watch()` or `poll_events()`.

#### Verification

With a `.dirsql.toml` holding a table whose `glob = "../absent/*.json"` and no `../absent` directory, `dirsql query "SELECT 1" -c .dirsql.toml` exits non-zero and prints `table '<name>': its directory .../absent does not exist`.
