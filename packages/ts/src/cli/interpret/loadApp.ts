// Dynamically import a user's `dirsql` config file (`.js` / `.mjs` /
// `.cjs`) and return its default export. The default export should be
// a constructed `DirSQL` instance; the caller relies on the runtime
// shape (no `instanceof` check), so any object that quacks like one
// will pass through.
//
// Throws if the dynamic `import()` fails or if the loaded module has
// no `default` export. The launcher in `cli/main.ts` catches both as
// startup failures.

import { pathToFileURL } from "node:url";
import type { DirSQL } from "../../index.js";

export async function loadApp(configPath: string): Promise<DirSQL> {
  const mod = await import(pathToFileURL(configPath).href);
  if (!mod.default) {
    throw new Error(
      `${configPath}: module must default-export a DirSQL instance`,
    );
  }
  return mod.default as DirSQL;
}
