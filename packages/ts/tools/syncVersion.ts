#!/usr/bin/env -S node --import tsx
// Rewrite `packages/ts/package.json` for a release: sets `version` and
// injects `optionalDependencies` from `PLATFORMS` (one
// `@dirsql/cli-<triple>@<version>` entry per target). Called from the
// publish-npm workflow between building the per-platform sub-packages
// and running `npm publish` on the main package.
//
// `optionalDependencies` are deliberately NOT committed to the
// package.json in source control: the `@dirsql/cli-*` sub-packages
// don't exist on npm until the first tagged release creates them, so
// having them in the committed manifest would break
// `pnpm install --frozen-lockfile` for contributors. Injecting at
// publish time solves both problems.
//
// Usage: `tsx tools/syncVersion.ts v0.2.0`
// Leading `v` is stripped.

import { readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { PLATFORMS } from "../ts/platforms.js";

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
  pkg.optionalDependencies = Object.fromEntries(
    PLATFORMS.map((p) => [p.name, version]),
  );
  writeFileSync(pkgPath, `${JSON.stringify(pkg, null, 2)}\n`);
  process.stdout.write(`synced package.json to ${version}\n`);
}

/* v8 ignore start -- script-invocation guard; tests import `syncVersion` directly */
if (!process.env.VITEST) {
  syncVersion(process.argv[2]);
}
/* v8 ignore stop */
