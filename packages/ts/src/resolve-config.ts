// Synchronous config resolver for `DirSQL.toJSON()`.
//
// Mirrors `DirSQLBuilder::resolve` in the Rust core: explicit options win
// for scalars; tables and ignore lists are concatenated; persist is OR-ed;
// path-valued config fields resolve relative to the config file's parent.

import { readFileSync } from "node:fs";
import { dirname, isAbsolute, resolve as resolvePath } from "node:path";
import { parse as parseToml } from "smol-toml";
import type { DirSQLConfig, DirSQLOptions } from "./index.js";

// biome-ignore lint/suspicious/noExplicitAny: TOML root has a dynamic shape.
type Cfg = Record<string, any>;

export function resolveConfig(options: DirSQLOptions): DirSQLConfig {
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
