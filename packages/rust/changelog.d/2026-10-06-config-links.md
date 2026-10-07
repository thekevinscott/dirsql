**Changed** A declared `[[table]] glob` skips `node_modules` unless the glob
names it, and follows symlinks, as a path-table does. `**/*.js` no longer lists
`node_modules/...`; `link.md -> top.md` and `linkdir/*` now list their rows.
