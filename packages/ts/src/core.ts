// Lazy access to the napi-rs core module.

import type { ExtensionSpec, RowEvent, ScanFailure } from "./dirsql.js";
import { loadNativeCore as defaultLoadNativeCore } from "./load-native-core.js";
import type { TableDef } from "./table.js";

// Shape of the napi-rs-exposed class. The `DirSQL` wrapper drives this.
export interface NativeDirSQL {
  query(sql: string): Promise<Record<string, unknown>[]>;
  startWatcher(): Promise<void>;
  pollEvents(timeoutMs: number): Promise<RowEvent[]>;
  // Synchronous: reads a list the scan already produced, no threadpool hop.
  scanFailures(): ScanFailure[];
  close(): void;
}

export interface NativeDirSQLConstructor {
  openAsync(
    root: string | null,
    tables: TableDef[] | null,
    ignore: string[] | null,
    config: string[] | null,
    persist: boolean | null,
    persistPath: string | null,
    extensions: ExtensionSpec[] | null,
    // Skip the core's own loading of the config's [[dirsql.extension]]
    // entries; set by the wrapper after resolving them itself.
    suppressConfigExtensions: boolean | null,
    noIgnore: boolean | null,
  ): Promise<NativeDirSQL>;
}

/// One config file handed to the core's extension planner: its absolute path,
/// and its contents, omitted when it could not be read.
export interface ConfigSource {
  path: string;
  contents?: string;
}

/** One planned `[[dirsql.extension]]` entry, as the core plans it.
 *
 * Exactly one of `path` and `package` is set: a `path` is ready to load, a
 * `package` must be located with `require.resolve` first — unless `shadow`
 * names an existing file, which takes precedence over the package.
 */
// napi omits a `None` field rather than emitting `null`, so the absent side
// of each variant is `undefined`.
export type ExtensionPlanEntry =
  | {
      path: string;
      package?: undefined;
      shadow?: undefined;
      entrypoint?: string;
    }
  | { path?: undefined; package: string; shadow: string; entrypoint?: string };

// Core module shape. The real implementation comes from the napi-rs
// native binary (`dirsql.node`); tests may substitute a fake.
export interface CoreModule {
  DirSQL: NativeDirSQLConstructor;
  configPathsFromArgv(argv: string[]): string[];
  // `null` when no config names an extension by package name: the core loads
  // every config's entries itself.
  planConfigExtensions(configs: ConfigSource[]): ExtensionPlanEntry[] | null;
  selectLoadable(name: string, dirs: string[], candidates: string[]): string;
  planExtensionPath(
    path: string,
    base: string,
    resolveRelative: boolean,
  ): ExtensionPlanEntry;
}

// Unit tests `vi.mock("./core.js")` to fake `getCore` directly, so
// production carries no test-only injection seam.
let core: CoreModule | null = null;

export function getCore(): CoreModule {
  if (core === null) {
    core = defaultLoadNativeCore() as CoreModule;
  }
  return core;
}
