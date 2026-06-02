// Fixture whose `extract` raises -- exercises the `ok: false` response.

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
      extract: () => {
        throw new Error("synthetic extract failure");
      },
    },
  ],
});
