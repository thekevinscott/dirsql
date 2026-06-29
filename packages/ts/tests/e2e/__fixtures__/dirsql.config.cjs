// A native CommonJS (.cjs) config file for dirsql. .cjs is always CJS,
// regardless of the nearest package.json's `type` field.
//
// When `dirsql --config <path-to-this-file>` runs, the Rust binary
// spawns `dirsql interpret` against this file. The helper loads the
// module, takes its `module.exports` value, and dispatches `extract`
// callbacks over NDJSON.

const { readFileSync } = require("node:fs");
const { join } = require("node:path");
const { DirSQL } = require("dirsql");

module.exports = new DirSQL({
  root: join(__dirname, "data"),
  tables: [
    {
      ddl: "CREATE TABLE papers (title TEXT)",
      glob: "**/meta.json",
      extract: (path) => [JSON.parse(readFileSync(path, "utf8"))],
    },
  ],
});
