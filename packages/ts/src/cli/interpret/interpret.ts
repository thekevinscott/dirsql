// Subcommand entry point for `dirsql interpret <config>`.
//
// Glues the per-purpose helpers (`loadApp`, `buildTables`,
// `dispatchExtract`, `writeMessage`) into the long-running NDJSON
// loop:
//
//   handshake (helper -> caller, once on startup):
//     {"type": "config", "state": <app.toJSON()>}
//
//   extract request (caller -> helper):
//     {"type": "extract", "id": <int>, "table": "<name>", "path": "<abs>"}
//
//   extract response (helper -> caller):
//     {"type": "result", "id": <int>, "ok": true,  "rows": [...]}
//     {"type": "result", "id": <int>, "ok": false, "error": "<msg>"}
//
// Exits 0 when stdin closes; non-zero with a single `dirsql interpret:`
// line on stderr if the config can't be loaded.

import { dirname, resolve as resolvePath } from "node:path";
import { createInterface } from "node:readline";
import type { DirSQL } from "../../index.js";
import { INTERPRET_ROOT_ENV } from "../../resolveConfig.js";
import { buildTables } from "./buildTables.js";
import { dispatchExtract } from "./dispatchExtract.js";
import { errMessage } from "./errMessage.js";
import { loadApp } from "./loadApp.js";
import { writeMessage } from "./writeMessage.js";

export async function interpret(configPath: string): Promise<number> {
  if (!configPath) {
    process.stderr.write("dirsql interpret: expected one config path, got 0\n");
    return 1;
  }

  // Expose the config file's parent directory as the implicit default root
  // for the user module we're about to import. A native config that omits
  // both `root` and `config` resolves its scan root to this value, matching
  // how a `.dirsql.toml` defaults its root (#251). Set before `loadApp` so
  // the user's `new DirSQL(...)` sees it during construction.
  process.env[INTERPRET_ROOT_ENV] = dirname(resolvePath(configPath));

  let app: DirSQL;
  try {
    app = await loadApp(configPath);
  } catch (e) {
    process.stderr.write(`dirsql interpret: ${errMessage(e)}\n`);
    return 1;
  }

  // The SDK constructor kicks off a background scan that calls each
  // table's `extract` for every matched file. If a user `extract`
  // throws, the scan rejects -- and since interpret never awaits the
  // scan (it dispatches to `extract` itself per request), the rejection
  // would surface as an unhandled promise rejection and crash Node.
  // Swallow it here; per-request errors are still reported to the
  // caller in the extract-response.
  app.ready.catch(() => {});

  const tables = buildTables(app);

  writeMessage({ type: "config", state: app.toJSON() });

  for await (const line of createInterface({ input: process.stdin })) {
    if (!line.trim()) {
      continue;
    }
    let req: { type?: string; id?: unknown; table?: string; path?: string };
    try {
      req = JSON.parse(line);
    } catch {
      continue;
    }
    if (!req || typeof req !== "object" || req.type !== "extract") {
      continue;
    }
    writeMessage(await dispatchExtract(req, tables));
  }

  return 0;
}
