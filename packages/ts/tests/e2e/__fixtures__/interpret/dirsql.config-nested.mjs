// Fixture: a native config that itself sets `config`. `dirsql interpret`
// must reject this -- a config file loaded by interpret cannot delegate to
// another config file. The referenced TOML is valid so the rejection comes
// from the loader, not a TOML read error.

import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { DirSQL } from "dirsql";

const here = dirname(fileURLToPath(import.meta.url));

export default new DirSQL({ config: join(here, "nested.dirsql.toml") });
