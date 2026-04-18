#!/usr/bin/env -S node --import tsx
// Build per-platform npm sub-packages from cargo-dist release archives.
//
// Inputs (env):
//   DIRSQL_DIST_DIR — archives directory (default: <repo>/target/distrib)
//   DIRSQL_VERSION  — version embedded in each package.json (default: read from packages/ts/package.json)
//   DIRSQL_OUT_DIR  — where to emit sub-package trees (default: <repo>/target/npm-platforms)
//
// Walks `PLATFORMS` and delegates each target to `buildOne()`.

import { mkdirSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { buildOne } from "./buildOne.js";
import { PLATFORMS } from "../ts/platforms.js";

export function buildPlatforms(): void {
  const tsPkg = resolve(import.meta.dirname, "..");
  const repo = resolve(tsPkg, "..", "..");

  const distDir = resolve(
    process.env.DIRSQL_DIST_DIR || join(repo, "target", "distrib"),
  );
  const outDir = resolve(
    process.env.DIRSQL_OUT_DIR || join(repo, "target", "npm-platforms"),
  );
  const version =
    process.env.DIRSQL_VERSION ||
    (JSON.parse(readFileSync(join(tsPkg, "package.json"), "utf8")) as {
      version: string;
    }).version;

  mkdirSync(outDir, { recursive: true });
  for (const p of PLATFORMS) {
    buildOne(p, distDir, outDir, version);
  }
}

/* v8 ignore start -- script-invocation guard; tests import `buildPlatforms` directly */
if (!process.env.VITEST) {
  buildPlatforms();
}
/* v8 ignore stop */
