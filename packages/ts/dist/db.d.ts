/**
 * In-memory SQLite database wrapper.
 *
 * Manages table creation with injected tracking columns,
 * row insertion/deletion by file path, and query execution
 * that strips internal columns from results.
 */
export type SqlValue = string | number | null | Buffer;
export interface Row {
    [column: string]: SqlValue;
}
/**
 * Parse the table name from a CREATE TABLE DDL statement.
 * Handles: CREATE TABLE name (...), CREATE TABLE IF NOT EXISTS name (...)
 */
export declare function parseTableName(ddl: string): string | null;
export declare class Db {
    private conn;
    constructor();
    /** Create a table from a user-provided DDL statement. */
    createTable(ddl: string): void;
    /** Insert a row into a table. */
    insertRow(table: string, row: Row, filePath: string, rowIndex: number): void;
    /** Delete all rows produced by a given file path. Returns number deleted. */
    deleteRowsByFile(table: string, filePath: string): number;
    /** Execute a SQL query, returning rows with internal columns stripped. */
    query(sql: string): Row[];
}
//# sourceMappingURL=db.d.ts.map