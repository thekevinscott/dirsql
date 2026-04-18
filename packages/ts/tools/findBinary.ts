// Walk an extracted archive looking for an entry with the given name.

import { readdirSync, statSync } from "node:fs";
import { join } from "node:path";

export function findBinary(rootDir: string, name: string): string | null {
  for (const entry of readdirSync(rootDir)) {
    const full = join(rootDir, entry);
    if (statSync(full).isDirectory()) {
      const hit = findBinary(full, name);
      if (hit) return hit;
    } else if (entry === name) {
      return full;
    }
  }
  return null;
}
