**Changed** Declared `[[table]]` tables respect `.gitignore` by default, as
path-tables do: inside a git repo, a file a `.gitignore` ignores is no longer a
row, at startup or while watching. `--no-ignore` turns this off for them too.
