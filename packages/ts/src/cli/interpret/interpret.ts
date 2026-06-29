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

import { createInterface } from "node:readline";
import type { DirSQL } from "../../index.js";
import { buildTables } from "./build-tables.js";
import { dispatchExtract } from "./dispatch-extract.js";
import { errMessage } from "./err-message.js";
import { loadApp } from "./load-app.js";
import { writeMessage } from "./write-message.js";

export async function interpret(configPath: string): Promise<number> {
  if (!configPath) {
    process.stderr.write("dirsql interpret: expected one config path, got 0\n");
    return 1;
  }

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
  // Swallow it here, before any early return, so a rejected scan never
  // escapes; per-request errors are still reported in the extract-response.
  app.ready.catch(() => {});

  // A config file describes a single DirSQL; it must not itself delegate to
  // another `config=` path. The interpret handshake has no field for a nested
  // config and would recurse, so reject it up front.
  if (app._options.config !== undefined) {
    process.stderr.write(
      "dirsql interpret: a config file cannot itself set config= " +
        "(nested config is not supported)\n",
    );
    return 1;
  }

  const tables = buildTables(app);

  // When the config supplies neither `root` nor `config=`, the resolved root
  // is "". Default it to the process cwd -- the directory the `dirsql` command
  // was launched from, which interpret inherits from the parent binary -- so a
  // root-less config indexes "here".
  const state = app.toJSON();
  if (!state.root) {
    state.root = process.cwd();
  }
  writeMessage({ type: "config", state });

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
