// Happy-path fixture for `dirsql interpret` integration tests.
//
// Exposes a single `papers` table whose `extract` reads each `meta.json`
// under the colocated `data/` tree and returns its parsed contents.

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
