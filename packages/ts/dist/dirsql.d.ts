/**
 * Main DirSQL class. Creates an in-memory SQLite index over a directory.
 *
 * TypeScript-idiomatic API:
 * - Constructor returns immediately, scan runs in background
 * - `await db.ready` waits for initial scan (awaitable property, not method)
 * - `db.query(sql)` executes queries synchronously (after ready)
 * - `db.watch()` returns an AsyncIterable of RowEvent
 */
import { type Row } from "./db.js";
import { type RowEvent } from "./differ.js";
/** A table definition for DirSQL. */
export interface Table {
    /** CREATE TABLE DDL statement */
    ddl: string;
    /** Glob pattern to match files for this table */
    glob: string;
    /** Extract rows from a file. Receives (relativePath, content) and returns rows. */
    extract: (path: string, content: string) => Row[];
}
export interface DirSQLOptions {
    /** Glob patterns to ignore */
    ignore?: string[];
}
export declare class DirSQL {
    private db;
    private root;
    private tableConfigs;
    private ignorePatterns;
    private fileRows;
    private matcher;
    private _readyPromise;
    private _initError;
    constructor(root: string, tables: Table[], options?: DirSQLOptions);
    /**
     * Awaitable property. Resolves when the initial scan is complete.
     * Throws if initialization failed. Can be awaited multiple times safely.
     */
    get ready(): Promise<void>;
    private _init;
    /** Execute a SQL query and return results as an array of row objects. */
    query(sql: string): Row[];
    /**
     * Watch for file changes. Returns an AsyncIterable of RowEvent.
     * The watcher starts immediately and yields events as files change.
     */
    watch(): AsyncIterable<RowEvent>;
}
//# sourceMappingURL=dirsql.d.ts.map