import type { ExtensionSpec } from "./dirsql.js";
import { hasBareName } from "./has-bare-name.js";
import { loadExtensionEntries } from "./load-extension-entries.js";
import { resolveEntries } from "./resolve-entries.js";

/**
 * Resolve a TOML config's `[[dirsql.extension]]` entries to literal paths.
 *
 * Returns the resolved specs — every entry resolved via
 * `resolveExtensionPath` against the config file's parent directory — when at
 * least one entry's `path` is a bare package name. Returns `null` when the
 * caller should not intervene: the config is missing, malformed, declares no
 * extensions, or uses only literal paths — leaving the core's own loading
 * (and error reporting) untouched. Throws if a package name cannot be
 * resolved.
 */
export function resolveConfigExtensionSpecs(
  configPath: string,
): ExtensionSpec[] | null {
  const loaded = loadExtensionEntries(configPath);
  if (loaded === null || !hasBareName(loaded.entries)) {
    return null;
  }
  return resolveEntries(loaded.entries, loaded.base);
}
