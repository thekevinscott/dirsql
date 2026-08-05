// Single source of truth for the target platforms `dirsql` publishes.
//
// Every target triple generates ONE npm sub-package: `@dirsql/lib-<slug>`,
// holding the napi-rs `.node` addon. It backs both layers — the TypeScript
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

export const PLATFORMS: readonly Platform[] = [
  {
    triple: "x86_64-unknown-linux-gnu",
    nodePlatform: "linux",
    nodeArch: "x64",
    libName: "@dirsql/lib-linux-x64-gnu",
    os: ["linux"],
    cpu: ["x64"],
    libc: ["glibc"],
  },
  {
    triple: "aarch64-unknown-linux-gnu",
    nodePlatform: "linux",
    nodeArch: "arm64",
    libName: "@dirsql/lib-linux-arm64-gnu",
    os: ["linux"],
    cpu: ["arm64"],
    libc: ["glibc"],
  },
  {
    triple: "x86_64-apple-darwin",
    nodePlatform: "darwin",
    nodeArch: "x64",
    libName: "@dirsql/lib-darwin-x64",
    os: ["darwin"],
    cpu: ["x64"],
  },
  {
    triple: "aarch64-apple-darwin",
    nodePlatform: "darwin",
    nodeArch: "arm64",
    libName: "@dirsql/lib-darwin-arm64",
    os: ["darwin"],
    cpu: ["arm64"],
  },
  {
    triple: "x86_64-pc-windows-msvc",
    nodePlatform: "win32",
    nodeArch: "x64",
    libName: "@dirsql/lib-win32-x64-msvc",
    os: ["win32"],
    cpu: ["x64"],
  },
];

/** Node `${platform}-${arch}` → `@dirsql/lib-*` napi sub-package name. */
export function libTriples(): Record<string, string> {
  const out: Record<string, string> = {};
  for (const p of PLATFORMS) {
    out[`${p.nodePlatform}-${p.nodeArch}`] = p.libName;
  }
  return out;
}

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
