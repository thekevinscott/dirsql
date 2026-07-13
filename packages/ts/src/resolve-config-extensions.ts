// SDK-side resolution of a TOML config's `[[dirsql.extension]]` entries.
//
// The Rust core loads config extensions literally — it has no
// `require.resolve`, so it cannot resolve a bare **package name**. When a
// config names an extension by package name, the SDK resolves every entry
// here, hands the core the resolved literal paths, and suppresses the core's
// own config-extension loading so the entries are not loaded twice.
//
// Shared by the `DirSQL` constructor (`config` option) and the CLI launcher.

import { existsSync, readFileSync } from "node:fs";
import { dirname, resolve as resolvePath } from "node:path";
import { parse as parseToml } from "smol-toml";
import type { ExtensionSpec } from "./dirsql.js";
import { isBareName, resolveExtensionPath } from "./resolve-extension.js";

// biome-ignore lint/suspicious/noExplicitAny: TOML root has a dynamic shape.
type Toml = Record<string, any>;

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
/** Load a config's `[[dirsql.extension]]` entries with its base directory.
 *
 * `null` when the config is missing, unreadable/malformed, or declares no
 * extension array — the caller leaves such configs to the core.
 */
function loadExtensionEntries(
  configPath: string,
): { entries: Toml[]; base: string } | null {
  if (!existsSync(configPath)) {
    return null;
  }
  let doc: Toml;
  try {
    doc = parseToml(readFileSync(configPath, "utf8")) as Toml;
  } catch {
    // Leave a malformed config for the core to report.
    return null;
  }
  const dirsql = (doc.dirsql ?? {}) as Toml;
  if (!Array.isArray(dirsql.extension)) {
    return null;
  }
  return { entries: dirsql.extension, base: dirname(resolvePath(configPath)) };
}

function hasBareName(entries: Toml[]): boolean {
  return entries.some((e) => typeof e.path === "string" && isBareName(e.path));
}

function resolveEntries(entries: Toml[], base: string): ExtensionSpec[] {
  return entries.map((e) => ({
    path: resolveExtensionPath(e.path as string, base, true),
    entrypoint: typeof e.entrypoint === "string" ? e.entrypoint : undefined,
  }));
}

export function resolveConfigExtensionSpecs(
  configPath: string,
): ExtensionSpec[] | null {
  const loaded = loadExtensionEntries(configPath);
  if (loaded === null || !hasBareName(loaded.entries)) {
    return null;
  }
  return resolveEntries(loaded.entries, loaded.base);
}

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
