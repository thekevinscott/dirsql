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
 * Thin class wrapper around {@link TableDef}, for parity with the Python and
 * Rust `Table` constructors. `new Table({...})` is structurally identical to
 * a plain object literal satisfying `TableDef` — anything accepting
 * `TableDef[]` takes either form.
 *
 * `strict` is only copied when present so the instance's enumerable keys
 * match the input literal exactly; field declarations use `declare` to
 * suppress the default class-field initializer that would otherwise pin
 * `strict` to `undefined` under `useDefineForClassFields`.
 */
export class Table implements TableDef {
  declare readonly ddl: string;
  declare readonly glob: string;
  declare readonly extract: (filePath: string) => Record<string, unknown>[];
  declare readonly strict?: boolean;

  constructor(def: TableDef) {
    this.ddl = def.ddl;
    this.glob = def.glob;
    this.extract = def.extract;
    if (def.strict !== undefined) {
      this.strict = def.strict;
    }
  }
}
