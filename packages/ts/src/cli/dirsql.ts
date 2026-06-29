#!/usr/bin/env node
// npm `bin` entry for `dirsql`. The package.json `bin` field points at
// the compiled version of this file; npm symlinks `node_modules/.bin/dirsql`
// to it. All it does is invoke `runCli()` (see `run-cli.ts`); the launcher
// logic and its error handling live there so they can be unit-tested.
//
// A prior `process.argv[1] === import.meta.filename` self-detection
// guard tripped up the npm-bin symlink: argv[1] is the (unresolved)
// symlink path, while import.meta.filename is the realpath, and the
// guard silently skipped main() so `dirsql --version` produced no
// output (caught by the pack-install build-CI smoke test).

import { runCli } from "./run-cli.js";

runCli();
