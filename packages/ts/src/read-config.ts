// Reading one TOML config off disk for the core's extension planner.

import { readFileSync } from "node:fs";

/** The config's text, or `undefined` when it cannot be read.
 *
 * A missing or unreadable config is left for the core to report.
 */
export function readConfig(configPath: string): string | undefined {
  try {
    return readFileSync(configPath, "utf8");
  } catch {}
}
