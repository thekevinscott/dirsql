// SDK-side resolution of the TOML configs' `[[dirsql.extension]]` entries.
//
// The planning — which configs parse, whether any entry names a package rather
// than a file, and what each literal path resolves to against its own config's
// directory — lives in the Rust core (`planConfigExtensions`). This module
// supplies the two host-specific halves the core cannot have: reading the
// files, and locating an installed package with `require.resolve`.
//
// Shared by the `DirSQL` constructor (`config` option) and the CLI launcher.

import { resolve as resolvePath } from "node:path";
import { getCore } from "./core.js";
import type { ExtensionSpec } from "./dirsql.js";
import { planEntryPath } from "./plan-entry-path.js";
import { readConfig } from "./read-config.js";

/**
 * Resolve the `[[dirsql.extension]]` entries of several configs, in order.
 *
 * The SDK intervenes for the whole set only when **some** config names an
 * extension by bare package name (the core can resolve neither package names
 * nor — once globally suppressed — the literal entries of the other configs).
 * When it intervenes it resolves **every** config's entries, each against that
 * config's own parent directory, concatenated in `configPaths` order. Returns
 * `null` when no config uses a package name, leaving every config's loading to
 * the core.
 */
export function resolveConfigsExtensionSpecs(
  configPaths: string[],
): ExtensionSpec[] | null {
  const plan = getCore().planConfigExtensions(
    configPaths.map((path) => ({
      path: resolvePath(path),
      contents: readConfig(path),
    })),
  );
  if (plan === null) {
    return null;
  }
  return plan.map((entry) => ({
    path: planEntryPath(entry),
    entrypoint: entry.entrypoint,
  }));
}
