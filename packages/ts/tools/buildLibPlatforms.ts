#!/usr/bin/env -S node --import tsx
// Build per-platform napi-rs sub-packages from prebuilt `.node` artifacts
// produced by the CI matrix build.
//
// Inputs (env):
//   DIRSQL_NAPI_DIR    — directory containing `dirsql.<slug>.node` files
//                        (default: <repo>/target/napi-artifacts)
//   DIRSQL_VERSION     — version embedded in each package.json
//                        (default: read from packages/ts/package.json)
//   DIRSQL_LIB_OUT_DIR — where to emit sub-package trees
//                        (default: <repo>/target/npm-lib-platforms)
//
// Walks `PLATFORMS` and delegates each target to `buildLibOne()`.

import { mkdirSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { buildLibOne } from "./buildLibOne.js";
import { PLATFORMS } from "../ts/platforms.js";

export function buildLibPlatforms(): void {
  const tsPkg = resolve(import.meta.dirname, "..");
  const repo = resolve(tsPkg, "..", "..");

  const nodeDir = resolve(
    process.env.DIRSQL_NAPI_DIR || join(repo, "target", "napi-artifacts"),
  );
  const outDir = resolve(
    process.env.DIRSQL_LIB_OUT_DIR || join(repo, "target", "npm-lib-platforms"),
  );
  const version =
    process.env.DIRSQL_VERSION ||
    (JSON.parse(readFileSync(join(tsPkg, "package.json"), "utf8")) as {
      version: string;
    }).version;

  mkdirSync(outDir, { recursive: true });
  for (const p of PLATFORMS) {
    buildLibOne(p, nodeDir, outDir, version);
  }
}

/* v8 ignore start -- script-invocation guard; tests import `buildLibPlatforms` directly */
if (!process.env.VITEST) {
  buildLibPlatforms();
}
/* v8 ignore stop */
