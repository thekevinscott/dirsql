// Glob a bare package name's platform loadable out of its installed directory.

import { readdirSync } from "node:fs";
import { join } from "node:path";
import { type PackageResolver, packageDir } from "./package-dir.js";
import { platformSuffixes } from "./platform-suffixes.js";

/** Glob the platform loadable inside a bare name's package dir. */
export function resolvePackage(
  name: string,
  resolver: PackageResolver,
): string {
  const dir = packageDir(name, resolver);
  const suffixes = platformSuffixes();
  const matches = (readdirSync(dir, { recursive: true }) as string[])
    .filter((entry) => suffixes.some((s) => entry.endsWith(s)))
    .map((entry) => join(dir, entry))
    .sort();

  const desc = suffixes.join(" / ");
  if (matches.length === 0) {
    throw new Error(
      `no loadable extension file (${desc}) found in package '${name}' (searched ${dir})`,
    );
  }
  if (matches.length > 1) {
    throw new Error(
      `multiple loadable extension files found in package '${name}': ${matches.join(", ")}; disambiguate with a literal path`,
    );
  }
  return matches[0] as string;
}
