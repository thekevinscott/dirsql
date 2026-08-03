import type { NativeDirSQL } from "./core.js";
import { getCore } from "./core.js";
import { resolveConfigsExtensionSpecs } from "./resolve-config-extensions.js";
import { resolveExtensionPath } from "./resolve-extension.js";
import type { TableDef } from "./table.js";

/**
 * A SQLite extension to load at startup, as accepted by the {@link DirSQL}
 * constructor's `extensions` option.
 */
export interface ExtensionSpec {
  /**
   * Path to the extension's shared library (`.so` / `.dylib` / `.dll`).
   * Taken verbatim — relative paths resolve against the process working
   * directory at load time (config-file paths, by contrast, resolve against
   * the config file's parent directory).
   */
  path: string;
  /**
   * Optional init-symbol override. When omitted, SQLite derives the entry
   * point from the filename, which often does not match — set this when the
   * extension's init function isn't `sqlite3_<filename>_init`.
   */
  entrypoint?: string;
}

/**
 * Options accepted by the {@link DirSQL} constructor.
 *
 * The index root is the explicit `root` when given, otherwise the process
 * working directory. The `config` file's location never sets the root.
 */
export interface DirSQLOptions {
  /** Root directory to scan. */
  root?: string;
  /** Programmatic table definitions. Each table's `onFile` runs in-process. */
  tables?: TableDef[];
  /** Glob patterns (relative to `root`) to ignore. */
  ignore?: string[];
  /**
   * Path to a `.dirsql.toml` config file, or an array of paths that merge in
   * order. Each config's `[[table]]` entries are appended to any programmatic
   * `tables`; its `[dirsql].ignore` patterns are appended to any explicit
   * `ignore`; a duplicate table name across configs errors. The config file's
   * location does not affect the index root — that is the explicit `root` when
   * given, otherwise the process working directory.
   */
  config?: string | string[];
  /**
   * Enable persistent on-disk SQLite cache. When `true`, the database is
   * written to `<root>/.dirsql/cache.db` (override via `persistPath`) so
   * subsequent startups only re-parse files that have actually changed.
   */
  persist?: boolean;
  /**
   * Override the location of the persistent cache file. Ignored when
   * `persist` is not `true`.
   */
  persistPath?: string;
  /**
   * SQLite extensions to load onto the connection at startup, before any
   * table DDL (enable → load → disable, so the SQL `load_extension()`
   * function is never left exposed). Programmatic entries load first, then
   * any `[[dirsql.extension]]` declared in `config`. A `path` (programmatic
   * or config-file) may be a bare **package name**, resolved from the
   * installed package under `node_modules`.
   */
  extensions?: ExtensionSpec[];
  /**
   * Opt path-table scans out of their default `.gitignore` respect, restoring
   * the gitignored files to scan results. The built-in floor (`node_modules`,
   * `.git`) and any `ignore` patterns still apply.
   */
  noIgnore?: boolean;
}

/** A row-level event emitted by the file watcher. */
export interface RowEvent {
  /**
   * The table the event applies to. Always set for insert / update / delete.
   * May be `null` on error events that occur before a file is attributed
   * to any table (e.g. a watch-channel failure).
   */
  table: string | null;
  action: "insert" | "update" | "delete" | "error";
  row?: Record<string, unknown> | null;
  oldRow?: Record<string, unknown> | null;
  error?: string | null;
  filePath?: string | null;
}

/**
 * One file the initial scan could not index.
 *
 * A scan failure is not a scan *error*: the other files are indexed and the
 * database is usable. This is how a caller learns the index is incomplete,
 * and which files are missing from it.
 */
export interface ScanFailure {
  /** Path relative to the scan root. */
  path: string;
  /** The hook's error, as it rendered it. */
  message: string;
}

/**
 * Ephemeral SQL index over a local directory.
 *
 * The constructor is overloaded: pass a config-file path directly, or an
 * options object with any combination of `root`, `tables`, `ignore`, and
 * `config`.
 *
 * Constructing a `DirSQL` returns immediately; the directory scan, file
 * reads, and initial row extraction run asynchronously. `db.ready`
 * resolves once construction has completed, and every method (including
 * {@link query}, {@link startWatcher}, {@link pollEvents}, and
 * {@link watch}) transparently awaits `ready` before doing any work, so
 * callers can start using the instance immediately:
 *
 * ```ts
 * // From a config file:
 * const db = new DirSQL("./my-config.toml");
 *
 * // Programmatic:
 * const db2 = new DirSQL({ root: "./data", tables: [...] });
 *
 * await db.ready; // optional: wait for the initial scan explicitly
 * const rows = await db.query("SELECT ...");
 * for await (const event of db.watch()) { ... }
 * ```
 *
 * The scan runs on the libuv threadpool, so constructing a `DirSQL` does
 * not block the JS event loop even for large directories.
 */
export class DirSQL {
  // The tracked construction promise. A no-op `.catch` is attached in the
  // constructor so a failure is never an unhandled rejection when the caller
  // constructs without awaiting `ready`; awaiters still observe the rejection.
  private readonly _ready: Promise<void>;

  /**
   * Resolves once the initial directory scan + row extraction have
   * completed, or rejects if construction failed. Every other method on
   * this class implicitly awaits `ready`, so awaiting it explicitly is
   * only necessary when a caller needs to observe construction errors
   * synchronously (without issuing a query first).
   */
  get ready(): Promise<void> {
    return this._ready;
  }

  // Initialized by `ready`. Do NOT touch before awaiting `ready`.
  private _inner!: NativeDirSQL;
  // Constructor options preserved verbatim; public-by-design.
  readonly _options: DirSQLOptions;

  /** Construct from a `.dirsql.toml` config-file path. */
  constructor(configPath: string);
  /** Construct from structured options. */
  constructor(options: DirSQLOptions);
  constructor(arg: string | DirSQLOptions) {
    const options: DirSQLOptions =
      typeof arg === "string" ? { config: arg } : arg;
    this._options = options;
    // A single path or an array; the array merges in order (each config's
    // [[table]] / ignore / [[dirsql.extension]] accumulate).
    const configPaths =
      options.config == null
        ? []
        : typeof options.config === "string"
          ? [options.config]
          : options.config;
    const Ctor = getCore().DirSQL;
    // Extension paths (possibly bare package names) are resolved inside the
    // promise chain so a resolution error rejects `ready` rather than
    // throwing from the constructor; the core accepts file paths only.
    this._ready = Promise.resolve()
      .then(() => {
        const extensions =
          options.extensions?.map((e) => ({
            path: resolveExtensionPath(e.path, process.cwd(), false),
            entrypoint: e.entrypoint,
          })) ?? null;
        // The SDK resolves the config's [[dirsql.extension]] entries itself
        // (bare package names need require.resolve, which the core lacks) and
        // suppresses the core's own config-extension loading so they are not
        // loaded twice.
        const configExtensions =
          configPaths.length > 0
            ? resolveConfigsExtensionSpecs(configPaths)
            : null;
        const merged =
          configExtensions !== null
            ? [...(extensions ?? []), ...configExtensions]
            : extensions;
        return Ctor.openAsync(
          options.root ?? null,
          options.tables ?? null,
          options.ignore ?? null,
          configPaths.length > 0 ? configPaths : null,
          options.persist ?? null,
          options.persistPath ?? null,
          merged,
          configExtensions !== null,
          options.noIgnore ?? null,
        );
      })
      .then((inner) => {
        this._inner = inner;
      });
    this._ready.catch(() => {});
  }

  /**
   * Execute a SQL query and return results as an array of row objects.
   *
   * Awaits the initial scan if it has not yet finished, then runs the
   * query on the libuv threadpool, so the JS event loop stays responsive
   * even for large result sets or long-running queries.
   */
  async query(sql: string): Promise<Record<string, unknown>[]> {
    await this.ready;
    return this._inner.query(sql);
  }

  /**
   * The files the initial scan could not index, each with its root-relative
   * `path` and the hook's own `message`.
   *
   * Empty after a clean scan, which is the signal to check: a non-empty list
   * means the index is *incomplete*, not wrong, and those files are retried
   * on the next scan.
   *
   * Awaits the initial scan first, so a caller cannot see an empty list
   * merely because the scan had not yet reached the failing file.
   */
  async scanFailures(): Promise<ScanFailure[]> {
    await this.ready;
    return this._inner.scanFailures();
  }

  /**
   * Start the file watcher. Must be called before {@link pollEvents}.
   * Idempotent — safe to call multiple times.
   *
   * Awaits the initial scan if it has not yet finished, then runs on the
   * libuv threadpool so the JS event loop stays responsive while the
   * watcher is being initialized.
   */
  async startWatcher(): Promise<void> {
    await this.ready;
    return this._inner.startWatcher();
  }

  /**
   * Poll for file change events, blocking up to `timeoutMs` for the first
   * event. Returns all events observed in the window (possibly empty).
   *
   * Awaits the initial scan if it has not yet finished, then runs on the
   * libuv threadpool so the JS event loop stays responsive for the
   * duration of the poll timeout.
   */
  async pollEvents(timeoutMs: number): Promise<RowEvent[]> {
    await this.ready;
    return this._inner.pollEvents(timeoutMs);
  }

  /**
   * Watch for file change events as an async iterable.
   *
   * ```ts
   * for await (const event of db.watch()) { ... }
   * ```
   *
   * Awaits the initial scan on first iteration, starts the underlying
   * watcher, then awaits a bounded native poll each cycle. The iterator
   * runs indefinitely; break out of the `for await` loop to stop.
   */
  async *watch(): AsyncGenerator<RowEvent, void, unknown> {
    await this.ready;
    await this._inner.startWatcher();
    while (true) {
      // Native poll runs on the libuv threadpool; ~200ms bounds each await
      // without starving the event loop.
      const events = await this._inner.pollEvents(200);
      for (const event of events) {
        yield event;
      }
    }
  }

  /**
   * Explicitly close the database connection. Used primarily for cleanup or
   * to ensure the persistent cache's WAL checkpoint completes (for testing).
   * After calling close, subsequent calls to query, watch, or other methods
   * will fail.
   */
  close(): void {
    this._inner.close();
  }
}
