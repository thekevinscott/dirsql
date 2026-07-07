#!/usr/bin/env node
// npm `bin` entry for `dirsql`; launcher logic and error handling live in
// `run-cli.ts` so they can be unit-tested.
//
// Do not add a `process.argv[1] === import.meta.filename` self-detection
// guard: under the npm-bin symlink argv[1] is the (unresolved) symlink path
// while import.meta.filename is the realpath, so the guard silently skips
// main().

import { runCli } from "./run-cli.js";

runCli();
