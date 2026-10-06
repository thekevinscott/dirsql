### Hidden `--include-default` flag removed

#### Summary

The internal, hidden `--include-default` flag (it seeded a placeholder `records` table ahead of any `-c` configs, and only the Python plugin launcher passed it) is removed along with the placeholder table. Passing it now fails as an unknown argument. No documented surface changes.

#### Required changes

| Surface | Before | After |
| ------- | ------ | ----- |
| `dirsql query "<sql>" --include-default -c plugin.toml` | placeholder `records` plus the config's tables | unknown argument; drop the flag |

#### Deprecations removed

The hidden `--include-default` flag.

#### Behavior changes without code changes

_None._

#### Verification

```bash
cd "$(mktemp -d)"
dirsql query "SELECT 1" --include-default
```

Expected: an unknown-argument error naming `--include-default`.
