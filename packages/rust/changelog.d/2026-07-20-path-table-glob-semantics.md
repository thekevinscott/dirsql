**Added** — path-table glob semantics. A directory is now recursive by default
(`'./'`, `'./docs'`), the non-recursive form is spelled explicitly (`'./*'`,
`'./docs/*'`), and a path naming a single file yields exactly one row. Absolute
(`'/var/log/*.log'`), parent-relative (`'../notes'`) and home-relative
(`'~/notes/*.md'`) path-tables now resolve instead of reporting that they are
unsupported; they report absolute `path` values, while `./` tables stay
relative to the index root. Path-table scans apply the configured `ignore`
patterns plus built-in `node_modules/**` and `.git/**` defaults, judged below
the literal part of the path you wrote — so `SELECT * FROM './'` skips
`node_modules`, but `'./node_modules/*/package.json'` still scans it. All three
SDKs and the CLI inherit this from the core.
