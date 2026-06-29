// Fixture for `dirsql interpret`: a native config that declares SQLite
// `extensions` (#230). Asserts the constructor option propagates into the
// handshake `state.extensions` array.
//
// Paths are absolute (taken verbatim by the SDK) and intentionally do not
// exist on disk: `interpret` only emits the synchronous `toJSON()`
// handshake and swallows the background scan's rejection, so no real
// shared library is loaded here. `entrypoint` is omitted on the second
// entry to exercise its normalization to `null`.

import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { DirSQL } from "dirsql";

const here = dirname(fileURLToPath(import.meta.url));

export default new DirSQL({
  root: join(here, "data"),
  extensions: [
    { path: "/opt/ext/vec0.so", entrypoint: "sqlite3_vec_init" },
    { path: "/opt/ext/spellfix.so" },
  ],
});
