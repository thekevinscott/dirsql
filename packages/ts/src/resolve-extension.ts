// Resolve an extension entry's `path` to a concrete loadable file.
//
// Resolution is an ordered probe (file-first, then package):
//
//   1. Path-looking (contains a separator, or ends in `.so` / `.dylib` /
//      `.dll` / `.node`) -> returned as a file path: made absolute against
//      `base` when `resolveRelative` is set (config-file entries), else
//      verbatim (programmatic entries).
//   2. Bare package name -> a same-named local file under `base` shadows the
//      package; otherwise the package dir is located via `require.resolve`
//      and the current platform's loadable is globbed from inside it. Zero
//      matches and multiple matches are both hard errors -- disambiguate
//      with a literal path.

import { existsSync, statSync } from "node:fs";
import { isAbsolute, resolve as resolvePath } from "node:path";
import { defaultResolver } from "./default-resolver.js";
import { isBareName } from "./is-bare-name.js";
import type { PackageResolver } from "./package-dir.js";
import { resolvePackage } from "./resolve-package.js";

/**
 * Resolve an extension `path` to a concrete file. `base` is the directory a
 * relative path and the bare-name shadow probe resolve against (a config file's
 * parent dir, or the cwd for programmatic entries). `resolveRelative` makes a
 * relative path-looking value absolute against `base` (config-file semantics);
 * when false it is returned verbatim (programmatic semantics).
 */
export function resolveExtensionPath(
  path: string,
  base: string,
  resolveRelative: boolean,
  resolver: PackageResolver = defaultResolver(),
): string {
  if (!isBareName(path)) {
    if (resolveRelative && !isAbsolute(path)) {
      return resolvePath(base, path);
    }
    return path;
  }
  const local = resolvePath(base, path);
  if (existsSync(local) && statSync(local).isFile()) {
    return local;
  }
  return resolvePackage(path, resolver);
}
