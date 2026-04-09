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
   * Create a new DirSQL instance that indexes a directory into an in-memory
   * SQLite database.  The initial scan runs in a microtask — await `db.ready`
   * before querying.
   *
   * @param root - Root directory path to index
   * @param tables - Array of table definitions
   * @param ignore - Optional array of glob patterns to ignore
   */
  constructor(root: string, tables: TableDef[], ignore?: string[]);

  /**
   * Create a DirSQL instance from a .dirsql.toml config file.
   *
   * @param configPath - Path to the .dirsql.toml config file
   * @returns A new DirSQL instance
   */
  static fromConfig(configPath: string): DirSQL;

  /**
   * Resolves when the initial directory scan is complete.
   * Rejects if the scan encountered an error.
   * Safe to await multiple times.
   */
  ready: Promise<void>;

  /**
   * Execute a SQL query against the in-memory database.
   *
   * @param sql - SQL query string
   * @returns Array of row objects
   */
  query(sql: string): Record<string, unknown>[];

  /**
   * Start watching for file changes.
   * Returns an async iterable of RowEvent objects.
   */
  watch(): AsyncIterable<RowEvent>;
}
