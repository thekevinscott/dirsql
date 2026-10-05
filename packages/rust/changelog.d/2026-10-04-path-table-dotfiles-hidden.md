**Changed** A path-table hides dot-named files and directories unless the
path spells them, the rule `ls` and `fd` use. `'./'` and `'./**'` skip
`.gitignore`, `.env` and everything under `.claude/`; `'./.claude/**'`,
`'./.env'` and `'./**/.env'` list them. A spelled component may be a glob
(`'./.env*'`, `'./.*'`). The rule is independent of `--no-ignore`, which
still governs `.gitignore` only. Declared `[[table]]` globs are unchanged.
