**Fixed** A path-table pattern that names `node_modules` after a glob
component now scans it: `'./**/node_modules/**'` lists every `node_modules`
in the tree and `'./*/node_modules/*/*'` the ones one level down. Only a
`node_modules` in the literal prefix used to count, so these returned nothing.
A pattern that does not name `node_modules`, such as `'./**'` or
`'./**/*.js'`, still skips it.
