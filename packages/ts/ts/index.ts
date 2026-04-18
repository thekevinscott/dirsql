// dirsql TypeScript SDK.
//
// The public surface is implemented in Rust via napi-rs. `pnpm build` runs
// `napi build` which produces `dirsql.node` at the package root, then
// `tsc` compiles this file to `dist/index.js` + `dist/index.d.ts`, which
// is what consumers import via the package's `main` / `types` / `exports`
// fields.
//
// The native binary lives at the package root (not in `dist/`) because
// that is where napi-rs writes it and where `napi prepublish` expects it.
// We resolve it relative to this file's location at runtime so the
// loader works whether the package is consumed via `node_modules/dirsql`
// or via a pnpm workspace self-reference from `test/`.

import { createRequire } from "node:module";
import { join } from "node:path";

/** Definition of a SQL-indexed table backed by files on disk. */
export interface TableDef {
  /** SQL DDL statement, e.g. `CREATE TABLE users (name TEXT, age INTEGER)`. */
  ddl: string;
  /** Glob pattern (relative to the DirSQL root) for files backing this table. */
  glob: string;
  /** Extract rows from a file's contents. Returns an array of row objects. */
  extract: (filePath: string, content: string) => Record<string, unknown>[];
  /** If true, reject rows with columns not declared in `ddl`. */
  strict?: boolean;
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

// Shape of the napi-rs-exposed class. The wrapper below drives this.
interface NativeDirSQL {
  query(sql: string): Promise<Record<string, unknown>[]>;
  startWatcher(): Promise<void>;
  pollEvents(timeoutMs: number): Promise<RowEvent[]>;
}

interface NativeDirSQLConstructor {
  new (root: string, tables: TableDef[], ignore?: string[]): NativeDirSQL;
  fromConfig(configPath: string): NativeDirSQL;
  openAsync(
    root: string,
    tables: TableDef[],
    ignore?: string[],
  ): Promise<NativeDirSQL>;
  fromConfigAsync(configPath: string): Promise<NativeDirSQL>;
}

// Core module shape. The real implementation comes from the napi-rs
// native binary (`dirsql.node`); tests may substitute a fake.
interface CoreModule {
  DirSQL: NativeDirSQLConstructor;
}

// Lazy-loaded reference to the core module. Populated on first access
// by `loadNativeCore()`, or by `__setCoreForTesting()` for tests.
let core: CoreModule | null = null;

/**
 * Load the native napi-rs binary. Resolved relative to this compiled
 * module: after `tsc` emits to `dist/`, the module's directory is
 * `<pkg>/dist`, so `..` reaches the package root where napi-rs writes
 * `dirsql.node`. The binary itself is a CommonJS addon, so we use
 * `createRequire` to load it from inside an ESM module.
 */
function loadNativeCore(): CoreModule {
  const bindingPath = join(import.meta.dirname, "..", "dirsql.node");
  const requireFromHere = createRequire(import.meta.url);
  return requireFromHere(bindingPath) as CoreModule;
}

function getCore(): CoreModule {
  if (core === null) {
    core = loadNativeCore();
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
 * Constructing a `DirSQL` returns immediately; the directory scan, file
 * reads, and initial row extraction run asynchronously. `db.ready`
 * resolves once construction has completed, and every method (including
 * {@link query}, {@link startWatcher}, {@link pollEvents}, and
 * {@link watch}) transparently awaits `ready` before doing any work, so
 * callers can start using the instance immediately:
 *
 * ```ts
 * const db = new DirSQL(root, tables);
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

  constructor(root: string, tables: TableDef[], ignore?: string[]) {
    const Ctor = getCore().DirSQL;
    const openPromise =
      ignore === undefined
        ? Ctor.openAsync(root, tables)
        : Ctor.openAsync(root, tables, ignore);
    this.ready = openPromise.then((inner) => {
      this._inner = inner;
    });
  }

  /**
   * Load a {@link DirSQL} instance from a `.dirsql.toml` config file.
   *
   * The root directory is derived from the config file's parent. Tables
   * are parsed using the built-in parser for each format declared in the
   * config. No JS `extract` callback is required.
   *
   * The config-driven path runs entirely on the libuv threadpool, so the
   * JS event loop stays responsive during the initial scan.
   */
  static async fromConfig(configPath: string): Promise<DirSQL> {
    const inner = await getCore().DirSQL.fromConfigAsync(configPath);
    const instance = Object.create(DirSQL.prototype) as DirSQL;
    const writable = instance as unknown as {
      _inner: NativeDirSQL;
      ready: Promise<void>;
    };
    writable._inner = inner;
    writable.ready = Promise.resolve();
    return instance;
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
