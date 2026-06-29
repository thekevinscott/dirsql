// Fixture: a native config that omits `root`. `dirsql interpret` should
// resolve the root to the helper process's current working directory rather
// than erroring. Mirrors `dirsql.config.mjs` but drops `root`.

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
