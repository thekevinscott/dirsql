// Build a `name -> TableDef` lookup the dispatcher can index by the
// `table` field on each extract request. Table name comes from the
// core's canonical `parseTableName` (Rust-side
// `dirsql::db::parse_table_name`) so the JS side and the orchestrator
// agree without a duplicate regex.

import { type DirSQL, type TableDef, parseTableName } from "../../index.js";

export function buildTables(app: DirSQL): Map<string, TableDef> {
  const out = new Map<string, TableDef>();
  for (const t of app._options.tables ?? []) {
    const name = parseTableName(t.ddl);
    if (name === null) {
      throw new Error(`could not parse table name from DDL: ${t.ddl}`);
    }
    out.set(name, t);
  }
  return out;
}
