### Core: path-table braces follow bash brace expansion

#### Summary

A path-table's braces now expand as bash expands them before globbing: a
group with a top-level comma is an alternation, a `..` range expands to each
value, and every other brace is literal text. `'./br/{q}.md'` used to match
`br/q.md` and now matches only a file named `br/{q}.md`; `'./a{1..3}.md'`
used to match nothing and now matches `a1.md`, `a2.md`, `a3.md`. Comma groups
behave as before. Every surface that resolves a path-table is affected (CLI,
REPL, HTTP server, SDKs, `--on-file`); config `[[table]] glob` is not.

#### Required changes

| Surface | Before | After |
| ------- | ------ | ----- |
| One-option group | `SELECT * FROM './br/{q}.md'` (matched `br/q.md`) | `SELECT * FROM './br/q.md'` |

#### Deprecations removed

_None._

#### Behavior changes without code changes

- A brace group with neither a top-level comma nor a valid `..` range is
  literal text.
- `{1..3}`, `{a..c}`, `{01..10}` and `{1..10..2}` expand to their values
  instead of matching the literal text between the braces.
- An unpaired `{` or `}` is literal text instead of a glob compile error.

#### Verification

```python
# root holds q.md, {q}.md, a1.md and a2.md
rows = await DirSQL(root).query("SELECT path FROM './{q}.md'")
# expected: [{"path": "{q}.md"}]
rows = await DirSQL(root).query("SELECT path FROM './a{1..2}.md' ORDER BY path")
# expected: [{"path": "a1.md"}, {"path": "a2.md"}]
```
