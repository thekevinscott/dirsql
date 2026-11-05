### Core: launcher config scan stops at `--`

#### Summary

The pip and npm launchers scan argv for `-c`/`--config` to resolve a config's
package-name extensions before the core runs. That scan now stops at the
first `--`, so a positional after it that starts with `-c` is no longer
mistaken for a config path. Nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

- An argument after `--` that starts with `-c` or `--config` no longer stops
  the launcher from resolving `./.dirsql.toml`'s package-name extensions.

#### Verification

```bash
python -c 'from dirsql._dirsql import config_paths_from_argv as f; print(f(["query", "--", "-cx"]))'
# expected: ['./.dirsql.toml']
```
