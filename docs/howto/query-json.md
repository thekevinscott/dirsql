# Query JSON file contents

A tree of JSON files is already a table — you just have to ask for it. The
hidden [`content`](../reference/path-tables.md#columns) column holds each
file's raw text, and SQLite's JSON operators read fields straight out of it.
No config, no parser, no schema.

## 1. Pull a field out of `content`

Suppose each plugin ships a `metadata.json`:

```
plugins/alpha/metadata.json
plugins/beta/metadata.json
plugins/gamma/metadata.json
```

```json
{
  "name": "alpha",
  "version": "1.4.0",
  "disabled": false,
  "runtime": { "engine": "wasm", "memory_mb": 64 },
  "tags": ["search", "index"]
}
```

The `->>` operator extracts a field. **The key is a string literal, not a bare
identifier** — this is the one thing that trips everyone up:

```bash
dirsql query "SELECT content ->> disabled FROM './plugins/alpha/metadata.json'"
```

```
dirsql query: SQLite error: no such column: disabled in SELECT content ->> disabled FROM './plugins/alpha/metadata.json' at offset 19
```

Unquoted, `disabled` is a column name to SQLite's parser, and no such column
exists. Quote it and the field comes back:

```bash
dirsql query "SELECT content ->> 'disabled' FROM './plugins/alpha/metadata.json'"
```

```json
[{"content ->> 'disabled'":0}]
```

The expression becomes the column name, so alias it:

```bash
dirsql query "SELECT content ->> 'disabled' AS disabled FROM './plugins/alpha/metadata.json'"
```

```json
[{"disabled":0}]
```

## 2. Query the whole tree

`content` is a [path-table](../reference/path-tables.md) column, so the same
expression works over a glob — one row per file:

```bash
dirsql query "SELECT content ->> 'name' AS name, content ->> 'version' AS version
              FROM './**/metadata.json' ORDER BY name"
```

```json
[{"name":"alpha","version":"1.4.0"},{"name":"beta","version":"0.9.2"},{"name":"gamma","version":"2.0.1"}]
```

`content` is read only when a query names it, so a glob over a large tree costs
nothing until you actually reach inside the files.

## 3. Reach nested keys

A key that starts with `$` is a **JSON path**: dotted for nested objects,
bracketed for array elements. It is still a string literal.

```bash
dirsql query "SELECT content ->> 'name' AS name,
                     content ->> '$.runtime.engine' AS engine,
                     content ->> '$.runtime.memory_mb' AS memory_mb
              FROM './**/metadata.json' ORDER BY name"
```

```json
[{"engine":"wasm","memory_mb":64,"name":"alpha"},{"engine":"native","memory_mb":256,"name":"beta"},{"engine":"wasm","memory_mb":128,"name":"gamma"}]
```

`content ->> 'name'` and `content ->> '$.name'` are the same lookup — the bare
form is shorthand for a top-level key. `json_extract(content, '$.name')` is the
function spelling of the same thing and takes the same paths.

## 4. Filter and aggregate

Extracted fields are ordinary SQL values, so they work in `WHERE`, `GROUP BY`,
and joins:

```bash
dirsql query "SELECT path FROM './**/metadata.json'
              WHERE content ->> 'disabled' = 0 ORDER BY path"
```

```json
[{"path":"plugins/alpha/metadata.json"},{"path":"plugins/gamma/metadata.json"}]
```

To count across an array field, expand it with `json_each`:

```bash
dirsql query "SELECT t.value AS tag, COUNT(*) AS n
              FROM './**/metadata.json' AS m, json_each(m.content, '$.tags') AS t
              GROUP BY tag ORDER BY n DESC, tag"
```

```json
[{"n":2,"tag":"index"},{"n":2,"tag":"search"},{"n":1,"tag":"export"},{"n":1,"tag":"rerank"}]
```

## `->` and `->>`

Both take the same paths; they differ in what they hand back. `->>` returns the
**SQL value**; `->` returns the **JSON text** of that value, quotes and all:

```bash
dirsql query "SELECT content -> 'name' AS arrow, content ->> 'name' AS arrow2
              FROM './plugins/alpha/metadata.json'"
```

```json
[{"arrow":"\"alpha\"","arrow2":"alpha"}]
```

Reach for `->>` to get a value out, and `->` to keep drilling — its JSON result
is a valid left operand for another `->` or `->>`.

## How JSON types arrive

`->>` maps JSON types onto SQLite's, which has no boolean:

| In the file | In the row |
| --- | --- |
| `true` / `false` | `1` / `0` |
| `null` | `NULL` |
| number | INTEGER or REAL |
| string | TEXT, unquoted |
| object or array | its JSON text |
| key not present | `NULL` |

So `disabled: false` reads back as `0`, and `WHERE content ->> 'disabled' = 0`
is how you ask for the enabled ones — `= false` is not SQLite syntax.

A missing key and a `null` value both arrive as `NULL`. Use
`content -> 'key' IS NULL` to tell them apart: `->` yields the JSON text
`'null'` for a present null, and SQL `NULL` only when the key is absent.

## When a file isn't valid JSON

The JSON operators reject a non-JSON `content`, and that fails the **whole
query** — one stray file, no rows at all:

```
dirsql query: SQLite error: malformed JSON
```

An empty file counts too: `content` is `''`, which is not valid JSON. Guard the
scan with `json_valid` when the glob might catch something it shouldn't:

```bash
dirsql query "SELECT content ->> 'name' AS name FROM './**/metadata.json'
              WHERE json_valid(content) ORDER BY name"
```

Files that fail the guard drop out and the rest still answer. A file that is
unreadable or not UTF-8 has `NULL` content, which is not an error —
`json_valid(NULL)` is `NULL`, so the guard excludes those too.

## Going further

- Only path-tables expose `content`. A named `[[table]]` has exactly the columns
  its hook emits ([columns reference](../reference/columns.md)) — reading a
  field with `->>` there means the hook already emitted it.
- Doing this on every query re-reads and re-parses every file. When the same
  tree is queried repeatedly, move the extraction into a table:
  [Extract rows from file contents](./extract-from-contents.md) declares the
  columns in a config, and [Parse your files into columns](./parse-files-into-columns.md)
  prototypes the same parser inline with `--on-file`. Either way the parsed rows
  are indexed, watched, and [persistable](./persist.md).
- One JSON file holding *many* records is a parser's job, not an operator's —
  `json_each` expands an array within a row, but one row per record needs
  [`on-file`](../reference/hooks.md#on-file).
- The full operator and function set is SQLite's own:
  [JSON functions](https://sqlite.org/json1.html). `dirsql` adds nothing to it
  and takes nothing away.
