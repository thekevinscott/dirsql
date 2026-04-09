/**
 * In-memory SQLite database wrapper.
 *
 * Manages table creation with injected tracking columns,
 * row insertion/deletion by file path, and query execution
 * that strips internal columns from results.
 */

import Database from "better-sqlite3";

export type SqlValue = string | number | null | Buffer;

export interface Row {
  [column: string]: SqlValue;
}

/**
 * Parse the table name from a CREATE TABLE DDL statement.
 * Handles: CREATE TABLE name (...), CREATE TABLE IF NOT EXISTS name (...)
 */
export function parseTableName(ddl: string): string | null {
  const upper = ddl.toUpperCase();
  const idx = upper.indexOf("CREATE TABLE");
  if (idx === -1) return null;

  let rest = ddl.slice(idx + "CREATE TABLE".length).trimStart();

  // Skip optional "IF NOT EXISTS"
  if (rest.toUpperCase().startsWith("IF NOT EXISTS")) {
    rest = rest.slice("IF NOT EXISTS".length).trimStart();
  }

  // Table name is everything up to the first whitespace or '('
  const match = rest.match(/^([^\s(]+)/);
  return match ? match[1] : null;
}

/**
 * Inject _dirsql_file_path and _dirsql_row_index columns into a CREATE TABLE DDL.
 */
function injectTrackingColumns(ddl: string): string {
  const lastParen = ddl.lastIndexOf(")");
  if (lastParen === -1) {
    throw new Error("DDL must contain a closing parenthesis");
  }
  const before = ddl.slice(0, lastParen);
  const after = ddl.slice(lastParen);
  return `${before}, _dirsql_file_path TEXT NOT NULL, _dirsql_row_index INTEGER NOT NULL${after}`;
}

export class Db {
  private conn: Database.Database;

  constructor() {
    this.conn = new Database(":memory:");
    this.conn.pragma("journal_mode = WAL");
  }

  /** Create a table from a user-provided DDL statement. */
  createTable(ddl: string): void {
    const augmented = injectTrackingColumns(ddl);
    this.conn.exec(augmented);
  }

  /** Insert a row into a table. */
  insertRow(
    table: string,
    row: Row,
    filePath: string,
    rowIndex: number,
  ): void {
    const columns = [...Object.keys(row), "_dirsql_file_path", "_dirsql_row_index"];
    const placeholders = columns.map(() => "?").join(", ");
    const sql = `INSERT INTO ${table} (${columns.join(", ")}) VALUES (${placeholders})`;
    const values = [...Object.values(row), filePath, rowIndex];
    this.conn.prepare(sql).run(...values);
  }

  /** Delete all rows produced by a given file path. Returns number deleted. */
  deleteRowsByFile(table: string, filePath: string): number {
    const sql = `DELETE FROM ${table} WHERE _dirsql_file_path = ?`;
    const result = this.conn.prepare(sql).run(filePath);
    return result.changes;
  }

  /** Execute a SQL query, returning rows with internal columns stripped. */
  query(sql: string): Row[] {
    const stmt = this.conn.prepare(sql);
    const rows = stmt.all() as Record<string, SqlValue>[];
    return rows.map((row) => {
      const filtered: Row = {};
      for (const [key, value] of Object.entries(row)) {
        if (!key.startsWith("_dirsql_")) {
          filtered[key] = value;
        }
      }
      return filtered;
    });
  }
}
