// Native CommonJS (.cjs) config fixture for dirsql. .cjs is always CJS,
// regardless of the nearest package.json's `type` field. Node >=22
// supports `require()` of the ESM-only `dirsql` package.

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
