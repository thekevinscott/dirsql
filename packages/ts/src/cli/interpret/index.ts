// Empty package shell — the native-config `interpret` subcommand was removed.
//
// The `interpret` NDJSON helper (`interpret`, `load-app`, `dispatch-extract`,
// `build-tables`, `err-message`, `write-message`) was removed in #321 (#324).
// The CLI now accepts only `.dirsql.toml`; to run user-defined `extract`
// callbacks, use the programmatic SDK (`new DirSQL(...)` with in-process
// closures).
//
// This file carries no logic and no re-exports. It remains only because the
// colocated-test tooling cannot yet express *deleting* an exempt barrel (the
// co-change check flags a deleted source that has no co-deleted colocated
// test, and a retained exempt for a deleted path is rejected as stale). The
// directory is removed once that is resolved.
export {};
