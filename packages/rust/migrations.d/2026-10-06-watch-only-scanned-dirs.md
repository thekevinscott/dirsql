### Watch mode skips ignored directories on Linux (#1401)

#### Summary

On Linux the live watcher registers one inotify watch per directory the scan enters, instead of one recursive watch over each root, so directories the `ignore` patterns skip take no watches. Nothing breaks: every file the index can hold still produces events.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

- On Linux, watch mode uses fewer inotify watches on trees holding ignored directories.

#### Verification

```bash
mkdir -p /tmp/w/node_modules/a/b/c /tmp/w/src
printf '[dirsql]\nignore = ["node_modules/**"]\n\n[[table]]\nname = "t"\nddl = "CREATE TABLE t (n INTEGER)"\nglob = "**/*.json"\non-file = "jq -c -s add"\n' > /tmp/w/.dirsql.toml
cd /tmp/w
dirsql server -c .dirsql.toml --port 47000 &
sleep 2; cat /proc/$!/fdinfo/* | grep -c '^inotify'   # 2: /tmp/w and /tmp/w/src, none under node_modules
```
