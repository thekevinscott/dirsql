### `dirsql context` is a subcommand (#1264)

#### Summary

`dirsql context` now prints the agent usage guide and exits 0. Before, the
word `context` as the first argument was run as SQL.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

- `dirsql context` used to be read as the SQL statement `context` and fail
  with a syntax error, exit 1. It now prints the guide.

#### Verification

```bash
dirsql context | head -n 1
# -> dirsql <version>
```
