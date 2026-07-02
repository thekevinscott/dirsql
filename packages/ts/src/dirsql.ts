// The `DirSQL` facade — the SDK's primary entry point.
//
// Split out of the public barrel (`index.ts`) so it carries a colocated
// unit test instead of an exemption (#239). The native binding is reached
// through `getCore()` (see `core.ts`), which tests fake.

import type { NativeDirSQL } from "./core.js";
import { getCore } from "./core.js";
import type { TableDef } from "./table.js";

/**
 * A SQLite extension to load at startup, as accepted by the {@link DirSQL}
 * constructor's `extensions` option. Mirrors the Rust `Extension { path,
 * entrypoint }` and the Python `{"path", "entrypoint"?}` dict.
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
 * At least one of `root` or `config` must be supplied. When both are set,
 * the explicit `root` wins over any `[dirsql].root` declared in the config
 * file (a warning is emitted by the native layer).
 */
export interface DirSQLOptions {
  /** Root directory to scan. */
  root?: string;
  /** Programmatic table definitions. Each table's `extract` runs in-process. */
  tables?: TableDef[];
  /** Glob patterns (relative to `root`) to ignore. */
  ignore?: string[];
  /**
   * Path to a `.dirsql.toml` config file. Its `[[table]]` entries are
   * appended to any programmatic `tables`; its `[dirsql].ignore` patterns
   * are appended to any explicit `ignore`. If the config declares a
   * `[dirsql].root` and no explicit `root` is given, it is resolved
   * relative to the config file's parent directory.
   */
  config?: string;
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
   * any `[[dirsql.extension]]` declared in `config`.
   */
  extensions?: ExtensionSpec[];
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
  /**
   * Resolves once the initial directory scan + row extraction have
   * completed, or rejects if construction failed. Every other method on
   * this class implicitly awaits `ready`, so awaiting it explicitly is
   * only necessary when a caller needs to observe construction errors
   * synchronously (without issuing a query first).
   */
  readonly ready: Promise<void>;

  // Initialized by `ready`. Do NOT touch before awaiting `ready`.
  private _inner!: NativeDirSQL;
  // Constructor options preserved verbatim for inspection without waiting on
  // `ready`. Public-by-design.
  readonly _options: DirSQLOptions;

  /** Construct from a `.dirsql.toml` config-file path. */
  constructor(configPath: string);
  /** Construct from structured options. */
  constructor(options: DirSQLOptions);
  constructor(arg: string | DirSQLOptions) {
    const options: DirSQLOptions =
      typeof arg === "string" ? { config: arg } : arg;
    this._options = options;
    const Ctor = getCore().DirSQL;
    const openPromise = Ctor.openAsync(
      options.root ?? null,
      options.tables ?? null,
      options.ignore ?? null,
      options.config ?? null,
      options.persist ?? null,
      options.persistPath ?? null,
      options.extensions ?? null,
    );
    this.ready = openPromise.then((inner) => {
      this._inner = inner;
    });
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
      // Native `pollEvents` now runs on the libuv threadpool and returns a
      // Promise, so awaiting it does not park the JS thread. A ~200ms
      // timeout keeps the poll cadence low without starving the event loop.
      const events = await this._inner.pollEvents(200);
      for (const event of events) {
        yield event;
      }
    }
  }
}
