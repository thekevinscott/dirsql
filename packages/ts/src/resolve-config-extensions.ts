// SDK-side resolution of a TOML config's `[[dirsql.extension]]` entries.
//
// The Rust core parses a `.dirsql.toml` itself and loads its extensions
// literally — it has no `require.resolve`, so it cannot resolve a bare
// **package name** (#227). The SDK can (#313). When a TOML config names an
// extension by package name, the SDK resolves every one of its extensions
// here, hands the core the resolved literal paths, and suppresses the core's
// own config-extension loading (the Rust `suppress_config_extensions` builder
// toggle) so the config's entries are not loaded a second time.
//
// Shared by the `DirSQL` constructor (`config` option) and the CLI launcher
// (`cli/resolve-config-extensions.ts`, which converts the resolved specs into
// `--extension` flags for the binary).

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
export function resolveConfigExtensionSpecs(
  configPath: string,
): ExtensionSpec[] | null {
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
  const entries: Toml[] = dirsql.extension;
  // Only intervene when at least one path is a bare package name; a config
  // with only literal paths (or no entries at all) keeps the core's existing
  // behavior untouched.
  const hasPackageName = entries.some(
    (e) => typeof e.path === "string" && isBareName(e.path),
  );
  if (!hasPackageName) {
    return null;
  }

  const base = dirname(resolvePath(configPath));
  return entries.map((e) => ({
    path: resolveExtensionPath(e.path as string, base, true),
    entrypoint: typeof e.entrypoint === "string" ? e.entrypoint : undefined,
  }));
}
