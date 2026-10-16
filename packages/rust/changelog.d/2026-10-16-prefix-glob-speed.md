**Changed**

- **One-shot queries over a glob under a prefix, such as `./a/b/**/*.md`, run faster.** `dirsql query` no longer starts an async worker per core, and on machines with more than sixteen cores the walk explores on half of them. Query results are unchanged.
