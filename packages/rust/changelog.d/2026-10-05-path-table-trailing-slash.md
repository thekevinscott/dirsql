**Fixed** A trailing `/` on a path-table is `*` appended. `'./*/'` now lists
the files directly inside each top-level directory, the same rows as
`'./*/*'` (like `ls */`); it used to drop the slash and list the top-level
files. `'./'`, `'./docs/'` and `'./**/'` return the same rows as before. The
rule applies to absolute, `../` and `~/` path-tables too.
