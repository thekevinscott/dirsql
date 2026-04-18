#!/usr/bin/env -S node --import tsx
// Rewrite a package.json (by default `packages/ts/package.json`) so that
// `version` and every entry in `optionalDependencies` match the given
// release tag. Called from the publish-npm workflow between building
// the per-platform sub-packages and running `npm publish` on the main
// package.
//
// Usage: `tsx tools/syncVersion.ts v0.2.0`
// Leading `v` is stripped.

import { readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";

export function defaultPkgPath(): string {
  return resolve(import.meta.dirname, "..", "package.json");
}

export function syncVersion(
  rawTag: string | undefined,
  pkgPath = defaultPkgPath(),
): void {
  if (!rawTag) {
    process.stderr.write("syncVersion: missing version argument\n");
    process.exit(1);
  }
  const version = rawTag.replace(/^v/, "");
  const pkg = JSON.parse(readFileSync(pkgPath, "utf8")) as {
    version: string;
    optionalDependencies?: Record<string, string>;
  };
  pkg.version = version;
  if (pkg.optionalDependencies) {
    for (const k of Object.keys(pkg.optionalDependencies)) {
      pkg.optionalDependencies[k] = version;
    }
  }
  writeFileSync(pkgPath, `${JSON.stringify(pkg, null, 2)}\n`);
  process.stdout.write(`synced package.json to ${version}\n`);
}

/* v8 ignore start -- script-invocation guard; tests import `syncVersion` directly */
if (!process.env.VITEST) {
  syncVersion(process.argv[2]);
}
/* v8 ignore stop */
