### Empty config tables warn on stderr (#1232)

#### Summary

A config `[[table]]` whose glob matches no files writes one line to stderr at startup. Nothing else changes: the query still runs and the exit code is the same.

#### Required changes

_None._ A script that treats any stderr output as failure should fix the glob or drop the table.

#### Deprecations removed

_None._

#### Behavior changes without code changes

- stderr gains `dirsql: table '<name>': glob '<glob>' matched no files under <anchor>` for each config table that matched nothing.

#### Verification

```bash
mkdir -p /tmp/cfg /tmp/elsewhere
printf '[[table]]\nname = "t"\nddl = "CREATE TABLE t (n TEXT)"\nglob = "nothing/*.jsonl"\non-file = "printf []"\n' > /tmp/cfg/.dirsql.toml
cd /tmp/elsewhere && dirsql query "SELECT COUNT(*) AS n FROM t" -c /tmp/cfg/.dirsql.toml   # stderr: dirsql: table 't': glob 'nothing/*.jsonl' matched no files under /tmp/cfg
```
