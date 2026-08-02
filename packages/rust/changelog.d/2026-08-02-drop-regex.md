**Changed** — the `regex` dependency is gone. `matcher.rs` used it for one
trivial pattern (`{name}` placeholder extraction in path templates); that is now
a hand-rolled scan over the same grammar, `{` `[a-zA-Z_][a-zA-Z0-9_]*` `}`.

Dropping the `regex` facade lets cargo drop the `regex-automata` features
`globset` does not need, shrinking every published artifact by ~570 KB
(CLI 6.14 -> 5.57 MB, pyo3 `.so` 4.99 -> 4.42 MB, napi `.node` 5.05 -> 4.48 MB;
~9-11% each, on top of the release-profile change).

Matcher construction also gets ~28x faster, since a `TableMatcher` no longer
compiles two regexes per pattern: 23.8 us -> 0.87 us for one pattern,
1.38 ms -> 47.8 us for fifty. Match throughput is unchanged (`globset` does the
matching).

No behavior change: `placeholder_names` keeps its signature and its exact
grammar, verified against the previous regex over 2,830 inputs with zero
mismatches.
