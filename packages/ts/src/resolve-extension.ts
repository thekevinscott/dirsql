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

import { existsSync, readdirSync, statSync } from "node:fs";
import { createRequire } from "node:module";
import { isAbsolute, join, resolve as resolvePath } from "node:path";
import { type PackageResolver, packageDir } from "./package-dir.js";

// Suffixes that mark a value as "already a file path" (so package resolution is
// never attempted).
const LOADABLE_SUFFIXES = [".so", ".dylib", ".dll", ".node"];

function defaultResolver(): PackageResolver {
  const req = createRequire(import.meta.url);
  return {
    resolve: (s) => req.resolve(s),
    paths: (s) => req.resolve.paths(s),
  };
}

/** Loadable-file glob suffix(es) for the current platform. */
function platformSuffixes(): string[] {
  if (process.platform === "darwin") {
    return [".dylib", ".node"];
  }
  if (process.platform === "win32") {
    return [".dll", ".node"];
  }
  return [".so", ".node"];
}

/** True when `path` is a bare package name rather than a file path. */
export function isBareName(path: string): boolean {
  if (path.includes("/") || path.includes("\\")) {
    return false;
  }
  return !LOADABLE_SUFFIXES.some((s) => path.endsWith(s));
}

/** Glob the platform loadable inside a bare name's package dir. */
function resolvePackage(name: string, resolver: PackageResolver): string {
  const dir = packageDir(name, resolver);
  const suffixes = platformSuffixes();
  const matches = (readdirSync(dir, { recursive: true }) as string[])
    .filter((entry) => suffixes.some((s) => entry.endsWith(s)))
    .map((entry) => join(dir, entry))
    .sort();

  const desc = suffixes.join(" / ");
  if (matches.length === 0) {
    throw new Error(
      `no loadable extension file (${desc}) found in package '${name}' (searched ${dir})`,
    );
  }
  if (matches.length > 1) {
    throw new Error(
      `multiple loadable extension files found in package '${name}': ${matches.join(", ")}; disambiguate with a literal path`,
    );
  }
  return matches[0] as string;
}

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
