// Read a TOML config's `[[dirsql.extension]]` array off disk.

import { existsSync, readFileSync } from "node:fs";
import { dirname, resolve as resolvePath } from "node:path";
import { parse as parseToml } from "smol-toml";

// biome-ignore lint/suspicious/noExplicitAny: TOML root has a dynamic shape.
export type Toml = Record<string, any>;

/** Load a config's `[[dirsql.extension]]` entries with its base directory.
 *
 * `null` when the config is missing, unreadable/malformed, or declares no
 * extension array — the caller leaves such configs to the core.
 */
export function loadExtensionEntries(
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
