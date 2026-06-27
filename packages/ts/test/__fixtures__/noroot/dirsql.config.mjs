// A native ESM (.mjs) config file with no explicit `root` (issue #251).
//
// When `dirsql --config <path-to-this-file>` runs, the Rust binary
// spawns `dirsql interpret` against this file. With `root` omitted, the
// scan root must default to this config file's parent directory --
// exactly as a `.dirsql.toml` does -- so the `data/**/meta.json` files
// below are indexed. Before the #251 fix, `new DirSQL({ tables })`
// serialized an empty/cwd root and the server returned HTTP 200 with an
// empty table (it scanned nothing).

import { readFileSync } from "node:fs";
import { DirSQL } from "dirsql";

export default new DirSQL({
  tables: [
    {
      ddl: "CREATE TABLE papers (title TEXT)",
      glob: "**/meta.json",
      extract: (path) => [JSON.parse(readFileSync(path, "utf8"))],
    },
  ],
});
