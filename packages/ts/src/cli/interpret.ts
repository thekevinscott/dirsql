// `dirsql interpret <config>` -- long-running native config helper (#196).
//
// Imports a JS/TS config file (.js / .mjs / .cjs), takes its default
// export (a `DirSQL` instance), and serves `extract` requests over
// NDJSON on stdin/stdout. One line in, one line out, sequential. The
// Rust orchestrator spawns this process when `--config` points to a
// native-language file.
//
// Protocol (one JSON object per line, flushed on every write):
//   handshake (helper -> caller, once on startup):
//     {"type": "config", "state": <app.toJSON()>}
//   extract request (caller -> helper):
//     {"type": "extract", "id": <int>, "table": "<name>", "path": "<abs>"}
//   extract response (helper -> caller):
//     {"type": "result", "id": <int>, "ok": true,  "rows": [...]}
//     {"type": "result", "id": <int>, "ok": false, "error": "<msg>"}
//
// Exits 0 when stdin closes; non-zero with a single `dirsql interpret:`
// line on stderr if the config can't be loaded.

import { createInterface } from "node:readline";
import { pathToFileURL } from "node:url";
import type { DirSQL, TableDef } from "../index.js";

// Pulls the table's SQL name out of `CREATE TABLE <name>` /
// `CREATE TABLE IF NOT EXISTS <name>` / quoted variants. `TableDef`
// doesn't carry a `name` field, so the request dispatcher derives it
// from `ddl`.
const NAME_RE = /^\s*CREATE\s+TABLE\s+(?:IF\s+NOT\s+EXISTS\s+)?["'`]?(\w+)/i;

function tableName(ddl: string): string {
  const m = NAME_RE.exec(ddl);
  if (!m) throw new Error(`could not parse table name from DDL: ${ddl}`);
  return m[1];
}

function writeLine(msg: unknown): void {
  process.stdout.write(`${JSON.stringify(msg)}\n`);
}

function errMessage(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

export async function interpret(configPath: string): Promise<number> {
  if (!configPath) {
    process.stderr.write("dirsql interpret: expected one config path, got 0\n");
    return 1;
  }

  let app: DirSQL;
  try {
    const mod = await import(pathToFileURL(configPath).href);
    if (!mod.default) {
      throw new Error(
        `${configPath}: module must default-export a DirSQL instance`,
      );
    }
    app = mod.default;
  } catch (e) {
    process.stderr.write(`dirsql interpret: ${errMessage(e)}\n`);
    return 1;
  }

  const tables = new Map<string, TableDef>(
    (app._options.tables ?? []).map((t) => [tableName(t.ddl), t]),
  );

  writeLine({ type: "config", state: app.toJSON() });

  for await (const line of createInterface({ input: process.stdin })) {
    if (!line.trim()) continue;
    let req: { type?: string; id?: unknown; table?: string; path?: string };
    try {
      req = JSON.parse(line);
    } catch {
      continue;
    }
    if (!req || typeof req !== "object" || req.type !== "extract") continue;
    const { id, table: name, path } = req;
    const table = name === undefined ? undefined : tables.get(name);
    if (!table) {
      writeLine({
        type: "result",
        id,
        ok: false,
        error: `unknown table: ${JSON.stringify(name)}`,
      });
      continue;
    }
    try {
      const rows = await table.extract(path as string);
      writeLine({ type: "result", id, ok: true, rows });
    } catch (e) {
      writeLine({ type: "result", id, ok: false, error: errMessage(e) });
    }
  }

  return 0;
}
