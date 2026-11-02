### Directories created mid-registration are watched

#### Summary

On Linux, the watcher walks a new directory again after watching it, until a walk finds nothing new, so a subdirectory created in between is watched too. Nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

Files written into a directory created in a burst of nested `mkdir`s now produce events where some were missed before.

#### Verification

```bash
dirsql init
dirsql server -c .dirsql.toml &
curl -sN http://localhost:7117/events &
mkdir -p a/b/c/d/e/f/g
echo '{"id":"1","name":"x"}' > a/b/c/d/e/f/g/r.json
```

Prints an insert for `a/b/c/d/e/f/g/r.json`.
