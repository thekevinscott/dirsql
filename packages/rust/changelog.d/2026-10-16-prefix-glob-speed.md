**Changed**

- **One-shot queries over a glob under a prefix, such as `./a/b/**/*.md`, run faster.** `dirsql query` no longer starts an async worker per core, and the walk starts its workers as a tree rather than all from one thread. Query results are unchanged.
