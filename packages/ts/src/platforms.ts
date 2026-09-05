// The target platforms `dirsql` publishes. The rows live in `platforms.json`,
// the one declarative copy of the table: this module types it, and the node
// distcheck flow (`internals/distcheck/.../node_flow/platforms.py`) reads the
// same file. The JSON sits under `src/` so `tsc` emits it into `dist/` and it
// ships with the package -- `loadNativeCore()` needs the map at runtime.
//
// Every target triple generates ONE npm sub-package: `@dirsql/lib-<slug>`,
// holding the napi-rs `.node` addon. It backs both layers -- the TypeScript
// SDK loads it via `loadNativeCore()`, and since #739 the `dirsql` CLI runs
// in-process through its `runCli` export rather than spawning a binary. The
// second family, `@dirsql/cli-<slug>`, shipped a redundant copy of the same
// core and is gone.
//
// The sub-packages are `optionalDependencies` of the main `dirsql` package,
// so npm/pnpm install only the one matching the host's OS/arch.
//
// `libTriples()` returns a `${process.platform}-${process.arch}` →
// sub-package-name map.

import rows from "./platforms.json" with { type: "json" };

export interface Platform {
  /** Rust target triple — the name cargo-dist uses for archives. */
  triple: string;
  /** Node `process.platform` value for this target. */
  nodePlatform: NodeJS.Platform;
  /** Node `process.arch` value for this target. */
  nodeArch: NodeJS.Architecture;
  /** napi library sub-package name (`@dirsql/lib-<slug>`). */
  libName: string;
  /** Wheel-style `os` constraint for the sub-package's package.json. */
  os: string[];
  /** Wheel-style `cpu` constraint for the sub-package's package.json. */
  cpu: string[];
  /** libc constraint (Linux only). */
  libc?: string[];
}

// JSON widens `nodePlatform` / `nodeArch` to `string`, which no data file can
// carry as `NodeJS.Platform` / `NodeJS.Architecture`. The cast is the only
// place the two vocabularies meet.
export const PLATFORMS = rows as readonly Platform[];

/** Node `${platform}-${arch}` → `@dirsql/lib-*` napi sub-package name. */
export function libTriples(): Record<string, string> {
  const out: Record<string, string> = {};
  for (const p of PLATFORMS) {
    out[`${p.nodePlatform}-${p.nodeArch}`] = p.libName;
  }
  return out;
}
