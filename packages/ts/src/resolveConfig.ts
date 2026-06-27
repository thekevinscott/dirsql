// Synchronous config resolver for `DirSQL.toJSON()`.
//
// Mirrors `DirSQLBuilder::resolve` in the Rust core: explicit options win
// for scalars; tables and ignore lists are concatenated; persist is OR-ed;
// path-valued config fields resolve relative to the config file's parent.

import { readFileSync } from "node:fs";
import { dirname, isAbsolute, resolve as resolvePath } from "node:path";
import { parse as parseToml } from "smol-toml";
import type { DirSQLConfig, DirSQLOptions } from "./index.js";

/**
 * Environment variable the `dirsql interpret` launcher sets, before
 * importing a user's native config module, to the config file's parent
 * directory. A native config that supplies neither `root` nor `config`
 * defaults its scan root to this value -- matching how a `.dirsql.toml`
 * defaults its root to the config's parent directory (#251). Outside the
 * interpret subprocess the variable is unset, so normal SDK use is
 * unaffected.
 */
export const INTERPRET_ROOT_ENV = "DIRSQL_INTERPRET_ROOT";

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

  // Precedence for `root`: explicit option > config-derived (`[dirsql].root`
  // or the config's parent dir) > the interpret launcher's implicit root.
  // The implicit root only applies when neither a root nor a config was
  // given -- i.e. a native config with no `root` (#251). Falls back to ""
  // (the prior behavior) when the var is unset, e.g. normal SDK use.
  const implicitRoot =
    options.config !== undefined
      ? typeof cfg.root === "string"
        ? abs(cfg.root)
        : cfgDir
      : (process.env[INTERPRET_ROOT_ENV] ?? "");

  return {
    root: options.root ?? implicitRoot,
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
