**Changed** Path-tables follow symlinks the way `bash -O globstar` does. A
symlinked file is listed like a regular file, including when the path names
it exactly. A symlinked directory is entered by any path component except
`**`, which never walks into one; `'./*/*'` reaches `linkdir/r.md`, `'./**'`
does not. A broken link lists nothing, and a symlink cycle cannot hang a scan.
Symlinks used to be skipped entirely. Declared `[[table]]` globs are
unchanged.
