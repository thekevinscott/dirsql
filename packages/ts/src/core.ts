// Lazy access to the napi-rs core module, plus the test-only seam used to
// swap in a fake binding without loading the real native binary.
//
// Split out of the public barrel (`index.ts`) so it carries a colocated
// unit test instead of an exemption (#239).

import type { ExtensionSpec, RowEvent } from "./dirsql.js";
import { loadNativeCore as defaultLoadNativeCore } from "./load-native-core.js";
import type { TableDef } from "./table.js";

// Shape of the napi-rs-exposed class. The `DirSQL` wrapper drives this.
export interface NativeDirSQL {
  query(sql: string): Promise<Record<string, unknown>[]>;
  startWatcher(): Promise<void>;
  pollEvents(timeoutMs: number): Promise<RowEvent[]>;
}

export interface NativeDirSQLConstructor {
  openAsync(
    root: string | null,
    tables: TableDef[] | null,
    ignore: string[] | null,
    config: string | null,
    persist: boolean | null,
    persistPath: string | null,
    extensions: ExtensionSpec[] | null,
    // Skip the core's own loading of the config's [[dirsql.extension]]
    // entries; set by the wrapper after resolving them itself (#313).
    suppressConfigExtensions: boolean | null,
  ): Promise<NativeDirSQL>;
}

// Core module shape. The real implementation comes from the napi-rs
// native binary (`dirsql.node`); tests may substitute a fake.
export interface CoreModule {
  DirSQL: NativeDirSQLConstructor;
  parseTableName(ddl: string): string | null;
}

// Lazy-loaded reference to the core module. Populated on first access by
// `defaultLoadNativeCore()`. Unit tests don't reach this state: they
// `vi.mock("./core.js")` to fake `getCore` directly, so production carries
// no test-only injection seam.
let core: CoreModule | null = null;

export function getCore(): CoreModule {
  if (core === null) {
    core = defaultLoadNativeCore() as CoreModule;
  }
  return core;
}
