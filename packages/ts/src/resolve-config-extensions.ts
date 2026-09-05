// SDK-side resolution of a TOML config's `[[dirsql.extension]]` entries.
//
// The Rust core loads config extensions literally — it has no
// `require.resolve`, so it cannot resolve a bare **package name**. When a
// config names an extension by package name, the SDK resolves every entry
// here, hands the core the resolved literal paths, and suppresses the core's
// own config-extension loading so the entries are not loaded twice.
//
// Shared by the `DirSQL` constructor (`config` option) and the CLI launcher.

import type { ExtensionSpec } from "./dirsql.js";
import { hasBareName } from "./has-bare-name.js";
import { loadExtensionEntries } from "./load-extension-entries.js";
import { resolveEntries } from "./resolve-entries.js";

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
  const loaded = configPaths.map(loadExtensionEntries);
  if (!loaded.some((item) => item !== null && hasBareName(item.entries))) {
    return null;
  }
  const specs: ExtensionSpec[] = [];
  for (const item of loaded) {
    if (item !== null) {
      specs.push(...resolveEntries(item.entries, item.base));
    }
  }
  return specs;
}
