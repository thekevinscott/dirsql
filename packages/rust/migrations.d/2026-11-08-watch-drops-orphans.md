### Watch leaves out the files that break a foreign key

#### Summary

On Linux, a watch batch that broke a foreign key used to roll back whole,
report one error, and replay with every later batch until one let it land.
Now only the offending files' rows are left out, each with its own error, and
the rest of the batch lands. No API changes; the row events and errors a watch
delivers differ. macOS and Windows keep the old behavior.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

- A batch with an orphaned row lands every other file's rows at once.
- Each file whose rows break a foreign key gets its own `Error` event naming
  that file. Its new rows are never announced; `Delete` events cover only
  rows of it that had landed before.
  Deleting a parent removes and reports each child file referencing it.
- A per-table hook whose rows break a foreign key keeps the rows it had,
  less any a deleted parent strands, and reports one `Error` naming its table
  with an empty file path. Its rejected rows are never announced. Every
  `on-file` table in a `.dirsql.toml` is such a hook.
- A left-out file is not re-admitted when its parent arrives later. It
  returns when its own file is rewritten or touched, when a directory holding
  it is renamed in, or on a rescan.

#### Verification

```bash
cd "$(mktemp -d)"
mkdir parents kids
echo '{"id": 1}' > parents/1.json
echo '{"pid": 1}' > kids/a.json
cat > .dirsql.toml <<'TOML'
[[table]]
name = "parent"
ddl  = "CREATE TABLE parent (id INTEGER PRIMARY KEY)"
glob = "parents/*.json"
on-file = "cat"

[[table]]
name = "child"
ddl  = "CREATE TABLE child (pid INTEGER REFERENCES parent(id))"
glob = "kids/*.json"
on-file = "cat"
TOML
dirsql server -c .dirsql.toml --port 47913 &
sleep 2
curl -sN http://127.0.0.1:47913/events &
sleep 1
echo '{"pid": 9}' > kids/b.json
echo '{"id": 2}' > parents/2.json
sleep 2
kill %2 %1
```

On Linux the event stream inserts `parents/2.json` and reports one `error`
for `child`, with no event for the `pid = 9` row.
