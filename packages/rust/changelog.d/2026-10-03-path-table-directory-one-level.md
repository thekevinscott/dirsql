**Changed** A path-table directory name is one level, like `ls`. `'./'`,
`'./docs'` and `'./docs/'` list the files directly inside that directory and
no deeper; `*` matches one level and `**` any depth, as in the shell, so the
recursive spellings are `'./**'` and `'./docs/**'`. The same rule applies to
absolute, `../` and `~/` path-tables and to the `--on-file` form. The
`did you mean FROM './'?` hint for the retired `files` table and the
hookless-`[[table]]` hint now point at `FROM './**'`.
