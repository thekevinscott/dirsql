// A native .js config file for dirsql. The launcher resolves .js as
// ESM or CJS based on the nearest package.json's `type` field; here the
// fixture sits inside an ESM package and is parsed as ESM.
//
// When `dirsql --config <path-to-this-file>` runs, the Rust binary
// spawns `dirsql interpret` against this file. The helper loads the
// module, takes its default export, and dispatches `extract` callbacks
// over NDJSON.

import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { DirSQL } from "dirsql";

const here = dirname(fileURLToPath(import.meta.url));

export default new DirSQL({
  root: join(here, "data"),
  tables: [
    {
      ddl: "CREATE TABLE papers (title TEXT)",
      glob: "**/meta.json",
      extract: (path) => [JSON.parse(readFileSync(path, "utf8"))],
    },
  ],
});
