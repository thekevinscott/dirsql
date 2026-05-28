// dirsql TypeScript SDK.
//
// The public surface is implemented in Rust via napi-rs. In development
// `pnpm build` runs `napi build` which drops `dirsql.node` at the
// package root; the loader in `loadNativeCore.ts` falls back to that
// file so running from source works.
//
// In a published install the napi binary ships inside a per-platform
// `@dirsql/lib-<slug>` sub-package (wired via `optionalDependencies`),
// and the loader resolves the one matching the host's OS/arch. No Rust
// toolchain is required at install time on any supported platform.

import { readFileSync } from "node:fs";
import { dirname, isAbsolute, resolve as resolvePath } from "node:path";
import { parse as parseToml } from "smol-toml";
import { loadNativeCore as defaultLoadNativeCore } from "./loadNativeCore.js";

/** Definition of a SQL-indexed table backed by files on disk. */
export interface TableDef {
  /** SQL DDL statement, e.g. `CREATE TABLE users (name TEXT, age INTEGER)`. */
  ddl: string;
  /** Glob pattern (relative to the DirSQL root) for files backing this table. */
  glob: string;
  /**
   * Produce the rows a matched file contributes. Receives the absolute
   * filesystem path of the file. dirsql does not read file contents; if the
   * callback needs the file body it reads the path itself (e.g.
   * `fs.readFileSync(filePath, "utf8")`). Returns an array of row objects.
   */
  extract: (filePath: string) => Record<string, unknown>[];
  /** If true, reject rows with columns not declared in `ddl`. */
  strict?: boolean;
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
 * Serializable per-table portion of {@link DirSQLConfig}. Excludes the
 * `extract` callback (closures aren't serializable) and the table's SQL
 * `name` (derivable from `ddl`).
 */
export interface TableConfig {
  ddl: string;
  glob: string;
  strict: boolean;
}

/**
 * Serializable snapshot of a {@link DirSQL} instance's resolved runtime
 * state, as produced by {@link DirSQL.toJSON} / `JSON.stringify(db)`.
 *
 * The shape is identical across the Python, Rust, and TypeScript SDKs
 * (modulo `persist_path` ↔ `persistPath` case): the same payload can flow
 * through the `interpret` handshake regardless of which SDK produced it.
 *
 * Construction artifacts that are no longer meaningful after the instance
 * exists are intentionally excluded — `config` (already merged into `root`
 * / `tables` / `ignore`), per-table `extract`, and per-table `name`.
 */
export interface DirSQLConfig {
  root: string;
  tables: TableConfig[];
  ignore: string[];
  persist: boolean;
  persistPath: string | null;
}

// Shape of the napi-rs-exposed class. The wrapper below drives this.
interface NativeDirSQL {
  query(sql: string): Promise<Record<string, unknown>[]>;
  startWatcher(): Promise<void>;
  pollEvents(timeoutMs: number): Promise<RowEvent[]>;
}

/**
 * Merge construction options with a `.dirsql.toml` into the serialized
 * state shape. Mirrors `DirSQLBuilder::resolve` in the Rust core: explicit
 * options win for scalars; tables and ignore lists are concatenated;
 * persist is OR-ed; path-valued config fields resolve relative to the
 * config file's parent.
 */
// biome-ignore lint/suspicious/noExplicitAny: TOML root has a dynamic shape.
type Cfg = Record<string, any>;
function resolveConfig(options: DirSQLOptions): DirSQLConfig {
  // When no config file is supplied, `cfg` is empty so `cfgDir` is never
  // read (the path-resolution helper is only reached via `cfg.root` /
  // `cfg.persist_path` lookups, both of which short-circuit on an empty cfg).
  let cfg: Cfg = {};
  let cfgTables: Cfg[] = [];
  let cfgDir = "";
  if (options.config !== undefined) {
    const doc = parseToml(readFileSync(options.config, "utf8")) as Cfg;
    cfg = (doc.dirsql ?? {}) as Cfg;
    cfgTables = (Array.isArray(doc.table) ? doc.table : []) as Cfg[];
    cfgDir = dirname(resolvePath(options.config));
  }
  const abs = (p: string) => (isAbsolute(p) ? p : resolvePath(cfgDir, p));

  return {
    root:
      options.root ?? (typeof cfg.root === "string" ? abs(cfg.root) : cfgDir),
    tables: [
      ...(options.tables ?? []).map((t) => ({
        ddl: t.ddl,
        glob: t.glob,
        strict: t.strict === true,
      })),
      ...cfgTables.map((e) => ({
        ddl: e.ddl as string,
        glob: e.glob as string,
        strict: e.strict === true,
      })),
    ],
    ignore: [...(options.ignore ?? []), ...((cfg.ignore ?? []) as string[])],
    persist: (options.persist ?? false) || cfg.persist === true,
    persistPath:
      options.persistPath ??
      (typeof cfg.persist_path === "string" ? abs(cfg.persist_path) : null),
  };
}

interface NativeDirSQLConstructor {
  openAsync(
    root: string | null,
    tables: TableDef[] | null,
    ignore: string[] | null,
    config: string | null,
    persist: boolean | null,
    persistPath: string | null,
  ): Promise<NativeDirSQL>;
}

// Core module shape. The real implementation comes from the napi-rs
// native binary (`dirsql.node`); tests may substitute a fake.
interface CoreModule {
  DirSQL: NativeDirSQLConstructor;
}

// Lazy-loaded reference to the core module. Populated on first access by
// `defaultLoadNativeCore()`, or by `__setCoreForTesting()` for tests.
let core: CoreModule | null = null;

function getCore(): CoreModule {
  if (core === null) {
    core = defaultLoadNativeCore() as CoreModule;
  }
  return core;
}

/**
 * **Test-only.** Replace the core module used by the SDK with a fake.
 *
 * This is an internal escape hatch for unit tests that want to mock the
 * napi-rs binding layer without loading the real native binary. Passing
 * `null` resets to the default (lazy native load on next access). Not
 * part of the public API; do not use in application code.
 */
export function __setCoreForTesting(fake: CoreModule | null): void {
  core = fake;
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
  // Constructor options preserved verbatim so `toJSON()` can resolve the
  // serialized state synchronously without waiting for `ready`.
  private readonly _options: DirSQLOptions;

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
   * Return a serializable snapshot of the resolved runtime state.
   *
   * Called automatically by `JSON.stringify(db)` (per the built-in JS
   * `toJSON()` protocol used by `Date`, `BigInt`, etc.). The shape mirrors
   * Python's `vars(db)` and Rust's `DirSQL::config()` so the same payload
   * can flow through the `interpret` handshake regardless of which SDK
   * produced it.
   *
   * Resolution -- including reading the `.dirsql.toml` if `config` was
   * supplied -- runs synchronously on each call, so this works immediately
   * after construction without awaiting `ready`.
   */
  toJSON(): DirSQLConfig {
    return resolveConfig(this._options);
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
