import { isBareName } from "./is-bare-name.js";
import type { Toml } from "./load-extension-entries.js";

/** True when some entry's `path` names an extension by bare package name. */
export function hasBareName(entries: Toml[]): boolean {
  return entries.some((e) => typeof e.path === "string" && isBareName(e.path));
}
