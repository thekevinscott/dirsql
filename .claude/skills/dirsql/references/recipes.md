# Recipes

Every query here runs as written from the directory in question. Output is a JSON array of row objects.

## Largest files

```dirsql
uvx dirsql "SELECT path, size FROM './**' ORDER BY size DESC LIMIT 10" --format json
```

## Count and total size by extension

```dirsql
uvx dirsql "SELECT ext, COUNT(*) AS files, SUM(size) AS bytes FROM './**' GROUP BY ext ORDER BY files DESC" --format json
```

`ext` is `NULL` for files with no extension and keeps its case; group by `LOWER(ext)` to fold `JPG` into `jpg`.

## Files per directory

```dirsql
uvx dirsql "SELECT dir, COUNT(*) AS files FROM './**' GROUP BY dir ORDER BY files DESC" --format json
```

`dir` is the empty string for files at the top level.

## Stale files

Not modified in the last 90 days:

```dirsql
uvx dirsql "SELECT path, datetime(mtime, 'unixepoch') AS modified FROM './**' WHERE mtime < strftime('%s', 'now') - 90 * 86400 ORDER BY mtime" --format json
```

Most recently modified markdown:

```dirsql
uvx dirsql "SELECT path, datetime(mtime, 'unixepoch') AS modified FROM './**/*.md' ORDER BY mtime DESC LIMIT 10" --format json
```

## The same JSON field from many files

```dirsql
uvx dirsql "SELECT path, content ->> 'name' AS name, content ->> 'version' AS version FROM './**/metadata.json' WHERE json_valid(content) ORDER BY name" --format json
```

`json_valid(content)` keeps one empty or malformed file from failing the whole query. Nested keys are JSON paths, still quoted: `content ->> '$.runtime.engine'`. Booleans arrive as `1`/`0`:

```dirsql
uvx dirsql "SELECT path FROM './**/metadata.json' WHERE json_valid(content) AND content ->> 'disabled' = 0" --format json
```

Count across an array field with `json_each`:

```dirsql
uvx dirsql "SELECT t.value AS tag, COUNT(*) AS n FROM './**/metadata.json' AS m, json_each(m.content, '$.tags') AS t GROUP BY tag ORDER BY n DESC, tag" --format json
```

## Frontmatter

Files whose frontmatter contains a given line (`%` matches across newlines):

```dirsql
uvx dirsql "SELECT path FROM './**/*.md' WHERE content LIKE '---%status: draft%'" --format json
```

Pull one frontmatter field out of every file:

```dirsql
uvx dirsql "WITH fm AS (SELECT path, substr(content, instr(content, 'title:') + 6) AS rest FROM './**/*.md' WHERE content LIKE '---%title:%') SELECT path, trim(substr(rest, 1, instr(rest, char(10)) - 1)) AS title FROM fm ORDER BY path" --format json
```

## Occurrences of a string per file

Where grep stops at matching lines, SQL aggregates:

```dirsql
uvx dirsql "SELECT path, (length(content) - length(replace(content, 'TODO', ''))) / length('TODO') AS todos FROM './**/*.md' WHERE todos > 0 ORDER BY todos DESC" --format json
```

## Join two directories

Source modules with no matching doc page:

```dirsql
uvx dirsql "SELECT s.path FROM './src/*.py' AS s LEFT JOIN './docs/**/*.md' AS d ON replace(d.basename, '.md', '') = replace(s.basename, '.py', '') WHERE d.path IS NULL ORDER BY s.path" --format json
```

## Top-k by meaning

Needs the embeddings plugin; the first run downloads a model (roughly a hundred megabytes, progress on stderr):

```sh
uvx --with dirsql-plugin-embeddings dirsql "
  SELECT path,
         vec_distance_cosine(emb, embed('how do I cook pasta?')) AS distance
  FROM (SELECT path, embed(content) AS emb FROM './notes/*.md')
  WHERE emb IS NOT NULL
  ORDER BY distance
  LIMIT 3" --format json
```

`WHERE emb IS NOT NULL` matters: an unreadable file embeds to `NULL`, and SQLite sorts `NULL` first. For ranked paths only, `uvx dirsql-plugin-embeddings './notes/*.md' "how do I cook pasta?" -k 3` prints `path<TAB>distance` lines.
