**Changed**

The TypeScript binding keeps queries reading the index as the watcher applied it, while the core now re-reads changed files at query time for Rust and Python: a TypeScript query cannot run a JavaScript `onFile` on the thread it runs on.
