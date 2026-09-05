import type { ExtensionSpec } from "./dirsql.js";
import type { Toml } from "./load-extension-entries.js";
import { resolveExtensionPath } from "./resolve-extension.js";

/** Resolve a config's entries to specs, against that config's own directory. */
export function resolveEntries(entries: Toml[], base: string): ExtensionSpec[] {
  return entries.map((e) => ({
    path: resolveExtensionPath(e.path as string, base, true),
    entrypoint: typeof e.entrypoint === "string" ? e.entrypoint : undefined,
  }));
}
