### TypeScript SDK: README table examples use the current `Table` signature

**Summary**

The README's table examples and the sentence introducing them now show
`{ name, ddl, glob, onFile }`. They showed `{ ddl, glob, extract }`, which does not type-check against `TableDef`.
README only: no code, config, or CLI surface changed, and nothing breaks.

**Required changes**

_None._

**Deprecations removed**

_None._

**Behavior changes without code changes**

_None._

**Verification**

```bash
node -e "import('dirsql').then(async ({ DirSQL }) => { console.log(await new DirSQL({ root: '.', tables: [{ name: 't', ddl: 'CREATE TABLE t (a TEXT)', glob: '*.nomatch', onFile: () => [] }] }).query('SELECT count(*) AS n FROM t')); process.exit(0); })"
# [ { n: 0 } ]
```
