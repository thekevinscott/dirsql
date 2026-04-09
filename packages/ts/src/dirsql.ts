/**
 * Main DirSQL class. Creates an in-memory SQLite index over a directory.
 *
 * TypeScript-idiomatic API:
 * - Constructor returns immediately, scan runs in background
 * - `await db.ready` waits for initial scan (awaitable property, not method)
 * - `db.query(sql)` executes queries synchronously (after ready)
 * - `db.watch()` returns an AsyncIterable of RowEvent
 */

import * as fs from "node:fs";
import * as path from "node:path";
import { watch as chokidarWatch, type FSWatcher } from "chokidar";
import { Db, parseTableName, type Row } from "./db.js";
import { diff, type RowEvent } from "./differ.js";
import { TableMatcher } from "./matcher.js";
import { scanDirectory } from "./scanner.js";

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

interface TableConfig {
  name: string;
  glob: string;
  extract: (path: string, content: string) => Row[];
}

export class DirSQL {
  private db: Db | null = null;
  private root: string;
  private tableConfigs: TableConfig[] = [];
  private ignorePatterns: string[];
  private fileRows: Map<string, [tableName: string, rows: Row[]]> = new Map();
  private matcher: TableMatcher | null = null;
  private _readyPromise: Promise<void>;
  private _initError: Error | null = null;

  constructor(root: string, tables: Table[], options?: DirSQLOptions) {
    this.root = path.resolve(root);
    this.ignorePatterns = options?.ignore ?? [];
    this._readyPromise = this._init(tables);
  }

  /**
   * Awaitable property. Resolves when the initial scan is complete.
   * Throws if initialization failed. Can be awaited multiple times safely.
   */
  get ready(): Promise<void> {
    return this._readyPromise;
  }

  private async _init(tables: Table[]): Promise<void> {
    try {
      this.db = new Db();

      // Parse and create tables
      const configs: TableConfig[] = [];
      for (const t of tables) {
        const name = parseTableName(t.ddl);
        if (!name) {
          throw new Error(`Could not parse table name from DDL: ${t.ddl}`);
        }
        this.db.createTable(t.ddl);
        configs.push({ name, glob: t.glob, extract: t.extract });
      }
      this.tableConfigs = configs;

      // Build matcher
      const mappings: Array<[string, string]> = configs.map((c) => [
        c.glob,
        c.name,
      ]);
      this.matcher = new TableMatcher(mappings, this.ignorePatterns);

      // Build extract lookup
      const extractMap = new Map<string, (p: string, c: string) => Row[]>();
      for (const c of configs) {
        extractMap.set(c.name, c.extract);
      }

      // Scan directory
      const files = scanDirectory(this.root, this.matcher);

      // Process each file
      for (const [filePath, tableName] of files) {
        const content = fs.readFileSync(filePath, "utf-8");
        const relPath = path.relative(this.root, filePath);
        const extractFn = extractMap.get(tableName);
        if (!extractFn) continue;

        const rows = extractFn(relPath, content);
        for (let i = 0; i < rows.length; i++) {
          this.db.insertRow(tableName, rows[i], relPath, i);
        }
        this.fileRows.set(relPath, [tableName, rows]);
      }
    } catch (err) {
      this._initError = err instanceof Error ? err : new Error(String(err));
      throw this._initError;
    }
  }

  /** Execute a SQL query and return results as an array of row objects. */
  query(sql: string): Row[] {
    if (!this.db) {
      throw new Error("DirSQL not initialized. Await db.ready first.");
    }
    return this.db.query(sql);
  }

  /**
   * Watch for file changes. Returns an AsyncIterable of RowEvent.
   * The watcher starts immediately and yields events as files change.
   */
  watch(): AsyncIterable<RowEvent> {
    if (!this.db || !this.matcher) {
      throw new Error("DirSQL not initialized. Await db.ready first.");
    }

    const db = this.db;
    const root = this.root;
    const matcher = this.matcher;
    const fileRows = this.fileRows;
    const tableConfigs = this.tableConfigs;

    // Build extract lookup
    const extractMap = new Map<string, (p: string, c: string) => Row[]>();
    for (const c of tableConfigs) {
      extractMap.set(c.name, c.extract);
    }

    return {
      [Symbol.asyncIterator](): AsyncIterableIterator<RowEvent> {
        const buffer: RowEvent[] = [];
        let resolve: (() => void) | null = null;
        let done = false;

        const watcher: FSWatcher = chokidarWatch(root, {
          ignoreInitial: true,
          awaitWriteFinish: {
            stabilityThreshold: 100,
            pollInterval: 50,
          },
        });

        function processFile(
          absPath: string,
          eventType: "add" | "change" | "unlink",
        ): void {
          const relPath = path.relative(root, absPath);

          if (matcher.isIgnored(relPath)) return;

          const tableName = matcher.matchFile(relPath);
          if (!tableName) return;

          const extractFn = extractMap.get(tableName);
          if (!extractFn) return;

          if (eventType === "unlink") {
            // File deleted
            const oldEntry = fileRows.get(relPath);
            const oldRows = oldEntry ? oldEntry[1] : null;
            const events = diff(tableName, oldRows, null, relPath);

            db.deleteRowsByFile(tableName, relPath);
            fileRows.delete(relPath);

            for (const ev of events) {
              buffer.push(ev);
            }
          } else {
            // File created or modified
            let content: string;
            try {
              content = fs.readFileSync(absPath, "utf-8");
            } catch (err: unknown) {
              if (
                err instanceof Error &&
                "code" in err &&
                (err as NodeJS.ErrnoException).code === "ENOENT"
              ) {
                return; // File disappeared between event and read
              }
              buffer.push({
                table: tableName,
                action: "error",
                row: null,
                oldRow: null,
                error: String(err),
                filePath: relPath,
              });
              return;
            }

            let newRows: Row[];
            try {
              newRows = extractFn(relPath, content);
            } catch (err) {
              buffer.push({
                table: tableName,
                action: "error",
                row: null,
                oldRow: null,
                error: `Extract error: ${err}`,
                filePath: relPath,
              });
              return;
            }

            const oldEntry = fileRows.get(relPath);
            const oldRows = oldEntry ? oldEntry[1] : null;
            const events = diff(tableName, oldRows, newRows, relPath);

            // Update DB
            db.deleteRowsByFile(tableName, relPath);
            for (let i = 0; i < newRows.length; i++) {
              db.insertRow(tableName, newRows[i], relPath, i);
            }
            fileRows.set(relPath, [tableName, newRows]);

            for (const ev of events) {
              buffer.push(ev);
            }
          }

          if (resolve) {
            const r = resolve;
            resolve = null;
            r();
          }
        }

        watcher.on("add", (p) => processFile(p, "add"));
        watcher.on("change", (p) => processFile(p, "change"));
        watcher.on("unlink", (p) => processFile(p, "unlink"));

        return {
          async next(): Promise<IteratorResult<RowEvent>> {
            if (done) {
              return { done: true, value: undefined };
            }

            while (buffer.length === 0) {
              await new Promise<void>((r) => {
                resolve = r;
              });
              if (done) {
                return { done: true, value: undefined };
              }
            }

            return { done: false, value: buffer.shift()! };
          },

          async return(): Promise<IteratorResult<RowEvent>> {
            done = true;
            if (resolve) {
              const r = resolve;
              resolve = null;
              r();
            }
            await watcher.close();
            return { done: true, value: undefined };
          },

          [Symbol.asyncIterator]() {
            return this;
          },
        };
      },
    };
  }
}
