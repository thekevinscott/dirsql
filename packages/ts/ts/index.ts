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
  table: string;
  action: "insert" | "update" | "delete" | "error";
  row?: Record<string, unknown> | null;
  oldRow?: Record<string, unknown> | null;
  error?: string | null;
  filePath?: string | null;
}

// Shape of the napi-rs-exposed class. The wrapper below drives this.
interface NativeDirSQL {
  query(sql: string): Record<string, unknown>[];
  startWatcher(): void;
  pollEvents(timeoutMs: number): RowEvent[];
}

interface NativeDirSQLConstructor {
  new (root: string, tables: TableDef[], ignore?: string[]): NativeDirSQL;
  fromConfig(configPath: string): NativeDirSQL;
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
 * module: after `tsc` emits to `dist/`, `__dirname` is `<pkg>/dist`, so
 * `..` reaches the package root where napi-rs writes `dirsql.node`.
 */
function loadNativeCore(): CoreModule {
  const bindingPath = join(__dirname, "..", "dirsql.node");
  // eslint-disable-next-line @typescript-eslint/no-require-imports
  return require(bindingPath) as CoreModule;
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
 * Constructing a `DirSQL` scans `root`, matches files against each
 * {@link TableDef}'s `glob`, extracts rows via `extract`, and builds an
 * in-memory SQLite database. `query` / `startWatcher` / `pollEvents`
 * are synchronous; {@link ready} and {@link watch} expose the same
 * surface in an async-idiomatic shape so TypeScript consumers don't
 * need a separate `AsyncDirSQL` class.
 *
 * ```ts
 * const db = new DirSQL(root, tables);
 * await db.ready;
 * const rows = db.query("SELECT ...");
 * for await (const event of db.watch()) { ... }
 * ```
 */
export class DirSQL {
  /**
   * Resolves once the initial directory scan has completed. Scanning
   * runs synchronously inside the constructor, so this Promise is
   * already resolved by the time the constructor returns; construction
   * failures throw synchronously rather than surfacing here. Exposed
   * as a Promise purely so consumers can write async-style code
   * uniformly across SDKs.
   */
  readonly ready: Promise<void>;

  private _inner: NativeDirSQL;

  constructor(root: string, tables: TableDef[], ignore?: string[]) {
    const Ctor = getCore().DirSQL;
    this._inner =
      ignore === undefined ? new Ctor(root, tables) : new Ctor(root, tables, ignore);
    this.ready = Promise.resolve();
  }

  /**
   * Load a {@link DirSQL} instance from a `.dirsql.toml` config file.
   *
   * The root directory is derived from the config file's parent. Tables
   * are parsed using the built-in parser for each format declared in the
   * config. No JS `extract` callback is required.
   */
  static fromConfig(configPath: string): DirSQL {
    const instance = Object.create(DirSQL.prototype) as DirSQL;
    const writable = instance as unknown as {
      _inner: NativeDirSQL;
      ready: Promise<void>;
    };
    writable._inner = getCore().DirSQL.fromConfig(configPath);
    writable.ready = Promise.resolve();
    return instance;
  }

  /** Execute a SQL query and return results as an array of row objects. */
  query(sql: string): Record<string, unknown>[] {
    return this._inner.query(sql);
  }

  /**
   * Start the file watcher. Must be called before {@link pollEvents}.
   * Idempotent — safe to call multiple times.
   */
  startWatcher(): void {
    this._inner.startWatcher();
  }

  /**
   * Poll for file change events, blocking up to `timeoutMs` for the first
   * event. Returns all events observed in the window (possibly empty).
   */
  pollEvents(timeoutMs: number): RowEvent[] {
    return this._inner.pollEvents(timeoutMs);
  }

  /**
   * Watch for file change events as an async iterable.
   *
   * ```ts
   * for await (const event of db.watch()) { ... }
   * ```
   *
   * Starts the underlying watcher on first iteration, then polls in 200ms
   * increments and yields each {@link RowEvent}. The iterator runs
   * indefinitely; break out of the `for await` loop to stop.
   */
  async *watch(): AsyncGenerator<RowEvent, void, unknown> {
    this._inner.startWatcher();
    while (true) {
      const events = this._inner.pollEvents(200);
      for (const event of events) {
        yield event;
      }
    }
  }
}
