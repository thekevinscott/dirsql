// Shared test helper: the `node:fs/promises` analog of `existsSync`.
// Kept in its own module (not a `.test.ts` file) so vitest doesn't
// collect it.

import { access } from "node:fs/promises";

/** True if `path` exists on disk. */
export async function exists(path: string): Promise<boolean> {
  try {
    await access(path);
    return true;
  } catch {
    return false;
  }
}
