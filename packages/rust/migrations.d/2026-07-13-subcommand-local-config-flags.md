### `dirsql query` config flags are subcommand-local (#609)

#### Summary

`-c`/`--config`, `--persist`, and `--extension` were clap `global` args,
accepted on either side of the `query` subcommand. A repeatable global does
**not** merge across the subcommand boundary — clap keeps the closest non-empty
context — so a `-c` straddling the subcommand was **silently dropped** (exit 0,
no warning). That is a silent-data-loss footgun the plugin launcher (#529) would
trip by injecting a `-c`. These flags are now **subcommand-local**: for `query`,
pass them **after** the subcommand; a flag placed *before* a subcommand is a hard
error. Server mode (no subcommand) is unchanged. Affects the `dirsql` CLI shipped
in the Rust crate and in the `pip` / `npm` launchers; no SDK signature changes.

#### Required changes

| Surface | Before | After |
| --- | --- | --- |
| Query with a config | `dirsql -c ./.dirsql.toml query "SELECT …"` | `dirsql query "SELECT …" -c ./.dirsql.toml` |
| Repeated configs on a query | `dirsql -c a -c b query "…"` | `dirsql query "…" -c a -c b` |
| `--persist` on a query | `dirsql --persist query "…"` | `dirsql query "…" --persist` |
| Server mode (no subcommand) | `dirsql -c ./.dirsql.toml` | unchanged |
| `dirsql init` output, then query it | `dirsql -c ./.dirsql.toml query "…"` | `dirsql query "…" -c ./.dirsql.toml` |

#### Deprecations removed

_None._

#### Behavior changes without code changes

- A config flag before the `query` subcommand now **errors**
  (`error: the subcommand 'query' cannot be used with '--config <CONFIG>'`,
  exit code 2) instead of parsing. Previously it either worked (flag on one
  side only) or was silently dropped (flag straddling both sides).
- Server mode (no subcommand) is unaffected:
  `dirsql -c <cfg> --host <addr> --port <n>` is unchanged.

#### Verification

In a directory whose `.dirsql.toml` defines a table named `posts`:

```console
$ dirsql query "SELECT COUNT(*) AS n FROM posts" -c ./.dirsql.toml
[{"n":<count>}]

$ dirsql -c ./.dirsql.toml query "SELECT 1"
error: the subcommand 'query' cannot be used with '--config <CONFIG>'
$ echo $?
2
```
