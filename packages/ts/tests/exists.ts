// Shared test helper: the `node:fs/promises` analog of `existsSync`.
//
// `node:fs/promises` has no `existsSync`; the idiom is `access()` in a
// try/catch. Kept in its own module (not a `.test.ts` file) so vitest
// doesn't collect it. Imported by the integration suite (`persist`) and
// the packaging smoke test (`smoke/build`).

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
