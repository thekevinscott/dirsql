export interface TableDef {
  /** SQL DDL statement (e.g., "CREATE TABLE users (name TEXT, age INTEGER)") */
  ddl: string;
  /** Glob pattern for matching files to this table */
  glob: string;
  /** Function to extract rows from a file */
  extract: (filePath: string, content: string) => Record<string, unknown>[];
  /** If true, reject rows with columns not in the DDL */
  strict?: boolean;
}

export interface RowEvent {
  table: string;
  action: "insert" | "update" | "delete" | "error";
  row?: Record<string, unknown> | null;
  oldRow?: Record<string, unknown> | null;
  error?: string | null;
  filePath?: string | null;
}

export class DirSQL {
  /**
   * Create a new DirSQL instance that indexes a directory into an in-memory SQLite database.
   *
   * @param root - Root directory path to index
   * @param tables - Array of table definitions
   * @param ignore - Optional array of glob patterns to ignore
   */
  constructor(root: string, tables: TableDef[], ignore?: string[]);

  /**
   * Execute a SQL query against the in-memory database.
   *
   * @param sql - SQL query string
   * @returns Array of row objects
   */
  query(sql: string): Record<string, unknown>[];

  /**
   * Start the file watcher. Must be called before pollEvents.
   */
  startWatcher(): void;

  /**
   * Poll for file change events.
   *
   * @param timeoutMs - Timeout in milliseconds to wait for events
   * @returns Array of row events (may be empty if no changes within timeout)
   */
  pollEvents(timeoutMs: number): RowEvent[];
}
