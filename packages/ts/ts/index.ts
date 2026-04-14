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

/**
 * Ephemeral SQL index over a local directory.
 *
 * Constructing a `DirSQL` scans `root`, matches files against each
 * {@link TableDef}'s `glob`, extracts rows via `extract`, and builds an
 * in-memory SQLite database. Call {@link DirSQL.query} to run SQL, or
 * {@link DirSQL.startWatcher} + {@link DirSQL.pollEvents} to react to
 * filesystem changes.
 */
export interface DirSQL {
  /** Execute a SQL query and return results as an array of row objects. */
  query(sql: string): Record<string, unknown>[];
  /** Start the file watcher. Must be called before {@link pollEvents}. */
  startWatcher(): void;
  /**
   * Poll for file change events.
   *
   * @param timeoutMs - Milliseconds to wait for events before returning.
   * @returns Array of row events; empty if no changes occurred in the window.
   */
  pollEvents(timeoutMs: number): RowEvent[];
}

/** Constructor shape for {@link DirSQL}. */
export interface DirSQLConstructor {
  new (root: string, tables: TableDef[], ignore?: string[]): DirSQL;
  /**
   * Load a {@link DirSQL} instance from a `.dirsql.toml` config file.
   *
   * The root directory is derived from the config file's parent. Tables
   * are parsed using the built-in parser for each format declared in the
   * config (`.json`, `.jsonl`, `.ndjson`, `.csv`, `.tsv`, `.toml`,
   * `.yaml`/`.yml`, `.md` frontmatter). No JS `extract` callback is
   * required or used. Honours `[dirsql].ignore` and per-table
   * `strict = true`.
   *
   * @param configPath - Path to the `.dirsql.toml` file.
   * @throws If the file is missing, the TOML is invalid, a table entry
   * lacks `ddl`/`glob`, or the format cannot be inferred.
   */
  fromConfig(configPath: string): DirSQL;
}

// Core module shape. The real implementation comes from the napi-rs
// native binary (`dirsql.node`); tests may substitute a fake.
interface CoreModule {
  DirSQL: DirSQLConstructor;
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
 * Public `DirSQL` export. Implemented as a Proxy so that
 * {@link __setCoreForTesting} can swap the underlying implementation
 * after this module has been imported. From a consumer's perspective it
 * behaves identically to the napi-rs-backed constructor: `new DirSQL(...)`
 * constructs an instance and `DirSQL.fromConfig(...)` is a static method.
 */
export const DirSQL: DirSQLConstructor = new Proxy(
  // Must be a `function` (not arrow) so the Proxy supports `construct`.
  // biome-ignore lint/complexity/useArrowFunction: Proxy construct trap requires a constructible target.
  function () {} as unknown as DirSQLConstructor,
  {
    construct(_target, args, newTarget) {
      const Ctor = getCore().DirSQL;
      return Reflect.construct(
        Ctor,
        args,
        newTarget === _target ? Ctor : newTarget,
      );
    },
    get(_target, prop, receiver) {
      return Reflect.get(getCore().DirSQL, prop, receiver);
    },
    has(_target, prop) {
      return prop in getCore().DirSQL;
    },
  },
);
