// The loadable path one planned extension entry resolves to.

import { existsSync, statSync } from "node:fs";
import type { ExtensionPlanEntry } from "./core.js";
import { defaultResolver } from "./default-resolver.js";
import type { PackageResolver } from "./package-dir.js";
import { resolvePackage } from "./resolve-package.js";

/** Resolve a planned entry to a concrete file.
 *
 * A literal entry is already resolved. A package entry prefers a same-named
 * file shadowing it next to the config, then the installed package.
 */
export function planEntryPath(
  entry: ExtensionPlanEntry,
  resolver: PackageResolver = defaultResolver(),
): string {
  if (entry.path !== undefined) {
    return entry.path;
  }
  if (existsSync(entry.shadow) && statSync(entry.shadow).isFile()) {
    return entry.shadow;
  }
  return resolvePackage(entry.package, resolver);
}
