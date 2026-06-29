// Lazy access to the napi-rs core module, plus the test-only seam used to
// swap in a fake binding without loading the real native binary.
//
// Split out of the public barrel (`index.ts`) so it carries a colocated
// unit test instead of an exemption (#239).

import type { RowEvent } from "./dirsql.js";
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
  ): Promise<NativeDirSQL>;
}

// Core module shape. The real implementation comes from the napi-rs
// native binary (`dirsql.node`); tests may substitute a fake.
export interface CoreModule {
  DirSQL: NativeDirSQLConstructor;
  parseTableName(ddl: string): string | null;
}

// Lazy-loaded reference to the core module. Populated on first access by
// `defaultLoadNativeCore()`, or by `__setCoreForTesting()` for tests.
let core: CoreModule | null = null;

export function getCore(): CoreModule {
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
