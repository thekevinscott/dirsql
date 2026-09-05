import { createRequire } from "node:module";
import type { PackageResolver } from "./package-dir.js";

/** `require.resolve`-backed resolver used when a caller injects none. */
export function defaultResolver(): PackageResolver {
  const req = createRequire(import.meta.url);
  return {
    resolve: (s) => req.resolve(s),
    paths: (s) => req.resolve.paths(s),
  };
}
