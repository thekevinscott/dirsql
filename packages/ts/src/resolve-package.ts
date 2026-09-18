// Locate a bare package name's platform loadable inside its installed dir.
//
// `require.resolve` finds the package's directory; the core picks the one
// loadable file inside it (and throws on zero or several — the caller must
// then disambiguate with a literal path).

import { readdirSync } from "node:fs";
import { join } from "node:path";
import { getCore } from "./core.js";
import { type PackageResolver, packageDir } from "./package-dir.js";

/** Pick the platform loadable inside a bare name's package dir. */
export function resolvePackage(
  name: string,
  resolver: PackageResolver,
): string {
  const dir = packageDir(name, resolver);
  const candidates = (readdirSync(dir, { recursive: true }) as string[]).map(
    (entry) => join(dir, entry),
  );
  return getCore().selectLoadable(name, [dir], candidates);
}
