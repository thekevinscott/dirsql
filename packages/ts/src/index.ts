// dirsql TypeScript SDK — public API barrel.
//
// The public surface is implemented in Rust via napi-rs. In development
// `pnpm build` runs `napi build` which drops `dirsql.node` at the
// package root; the loader in `load-native-core.ts` falls back to that
// file so running from source works.
//
// In a published install the napi binary ships inside a per-platform
// `@dirsql/lib-<slug>` sub-package (wired via `optionalDependencies`),
// and the loader resolves the one matching the host's OS/arch. No Rust
// toolchain is required at install time on any supported platform.
//
// This file is a thin re-export barrel: the implementation lives in
// colocated-tested modules (`table.ts`, `core.ts`, `parse-table-name.ts`,
// `dirsql.ts`), each carrying its own unit test. The barrel's re-export
// statements are covered by `index.test.ts`.

export { DirSQL } from "./dirsql.js";
export type {
  DirSQLOptions,
  ExtensionSpec,
  RowEvent,
} from "./dirsql.js";
export { parseTableName } from "./parse-table-name.js";
export { Table } from "./table.js";
export type { TableDef } from "./table.js";
