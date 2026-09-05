// Locate an installed package's directory from a bare name, for extension
// resolution.

import { existsSync, statSync } from "node:fs";
import { join } from "node:path";

/**
 * `require.resolve`-shaped seam. Injectable so unit tests can fake module
 * resolution without a real `node_modules` layout.
 */
export interface PackageResolver {
  /** Resolve a specifier to an on-disk path (`require.resolve`). */
  resolve(specifier: string): string;
  /** Candidate `node_modules` dirs for a specifier (`require.resolve.paths`). */
  paths(specifier: string): string[] | null;
}

/** Locate the on-disk package directory for a bare name. */
export function packageDir(name: string, resolver: PackageResolver): string {
  // The package.json's directory is the package root. Preferred because it is
  // unaffected by an `exports` map that hides the main entry.
  try {
    const pkgJson = resolver.resolve(`${name}/package.json`);
    return pkgJson.slice(0, pkgJson.length - "/package.json".length);
  } catch {
    // `exports` may forbid resolving package.json; fall back to scanning the
    // candidate node_modules dirs for `<dir>/<name>`.
  }
  for (const dir of resolver.paths(name) ?? []) {
    const candidate = join(dir, name);
    if (existsSync(candidate) && statSync(candidate).isDirectory()) {
      return candidate;
    }
  }
  throw new Error(
    `could not resolve extension package '${name}': not installed`,
  );
}
