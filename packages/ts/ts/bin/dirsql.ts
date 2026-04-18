#!/usr/bin/env node
// npm `bin` entry for `dirsql`. Runs the launcher when invoked from the
// command line; doesn't do anything when imported (so tests can pull in
// individual modules without side effects).

import { main } from "./main.js";

// `import.meta.filename` (Node 20.11+) avoids the
// `dirname(fileURLToPath(import.meta.url))` boilerplate. When this
// module is the script Node was invoked with, run the launcher.
if (process.argv[1] === import.meta.filename) {
  main();
}
