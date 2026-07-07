import { getCore } from "./core.js";

/**
 * Parse the table name out of a `CREATE TABLE <name> (...)` DDL.
 *
 * Returns `null` for a DDL the parser doesn't recognize. Backed by the
 * core Rust implementation (`dirsql::db::parse_table_name`) so the JS
 * SDK and the orchestrator agree on table-name resolution.
 */
export function parseTableName(ddl: string): string | null {
  return getCore().parseTableName(ddl);
}
