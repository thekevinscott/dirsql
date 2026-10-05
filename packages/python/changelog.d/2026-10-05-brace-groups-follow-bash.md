**Changed** Braces in a path-table follow bash brace expansion. A group with
a top-level comma (`{a,b}`, nested allowed) is an alternation, and a `..`
range (`{1..3}`, `{a..c}`, `{01..10}`, `{1..10..2}`) expands to each value.
Any other brace is literal text: `'./br/{q}.md'` names the file `br/{q}.md`
where it used to match `br/q.md`, and `{` or `}` without a partner no longer
fails to compile. Config `[[table]] glob` is unchanged.
