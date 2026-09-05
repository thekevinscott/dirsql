import type { Platform } from "./platforms.js";

/**
 * Suffix used in the napi `.node` filename for a given triple. Follows the
 * `@napi-rs/cli` convention: `dirsql.<platform>-<arch>[-<abi>].node`, e.g.
 * `dirsql.linux-x64-gnu.node`. Derived from the sub-package name so the
 * on-disk artifact name and the npm package name can't drift.
 */
export function librarySlug(p: Platform): string {
  const prefix = "@dirsql/lib-";
  if (!p.libName.startsWith(prefix)) {
    throw new Error(`libName ${p.libName} missing ${prefix} prefix`);
  }
  return p.libName.slice(prefix.length);
}
