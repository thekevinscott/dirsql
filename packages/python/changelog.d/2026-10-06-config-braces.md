**Changed** In a declared `[[table]] glob`, `{name}` is a literal, not a
wildcard, as in a path-table and in bash. The load-time error for a `{name}`
that collides with a declared column is gone. Write `*` for the old meaning.
