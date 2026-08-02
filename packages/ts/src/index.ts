// dirsql TypeScript SDK — public API barrel.

export { DirSQL } from "./dirsql.js";
export type {
  DirSQLOptions,
  ExtensionSpec,
  RowEvent,
  ScanFailure,
} from "./dirsql.js";
export { parseTableName } from "./parse-table-name.js";
export { Table } from "./table.js";
export type { TableDef } from "./table.js";
