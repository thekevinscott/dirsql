// Single source of truth for the target platforms `dirsql` publishes.
//
// Every target triple generates two npm sub-packages:
//
// 1. `@dirsql/cli-<slug>` — holds the standalone `dirsql` CLI binary
//    (from cargo-dist). Consumed at runtime by `ts/bin/resolveBinary.ts`
//    when a user runs the `dirsql` CLI.
// 2. `@dirsql/lib-<slug>` — holds the napi-rs `.node` addon used by the
//    TypeScript SDK. Consumed at runtime by `loadNativeCore()` in
//    `ts/index.ts` when a user `import`s from `dirsql`.
//
// Both sub-package sets use `optionalDependencies` on the main `dirsql`
// package so npm/pnpm install only the one matching the host's OS/arch.
//
// `nodeTriples()` / `libTriples()` return `${process.platform}-${process.arch}`
// → sub-package-name maps for the respective layer.

export interface Platform {
  /** Rust target triple — the name cargo-dist uses for archives. */
  triple: string;
  /** Node `process.platform` value for this target. */
  nodePlatform: NodeJS.Platform;
  /** Node `process.arch` value for this target. */
  nodeArch: NodeJS.Architecture;
  /** CLI sub-package name (`@dirsql/cli-<slug>`). */
  name: string;
  /** napi library sub-package name (`@dirsql/lib-<slug>`). */
  libName: string;
  /** Wheel-style `os` constraint for the sub-package's package.json. */
  os: string[];
  /** Wheel-style `cpu` constraint for the sub-package's package.json. */
  cpu: string[];
  /** libc constraint (Linux only). */
  libc?: string[];
  /** Archive extension cargo-dist emits for this target. */
  ext: "tar.xz" | "zip";
  /** Whether the binary has a `.exe` suffix on this platform. */
  exe?: boolean;
}

export const PLATFORMS: readonly Platform[] = [
  {
    triple: "x86_64-unknown-linux-gnu",
    nodePlatform: "linux",
    nodeArch: "x64",
    name: "@dirsql/cli-linux-x64-gnu",
    libName: "@dirsql/lib-linux-x64-gnu",
    os: ["linux"],
    cpu: ["x64"],
    libc: ["glibc"],
    ext: "tar.xz",
  },
  {
    triple: "aarch64-unknown-linux-gnu",
    nodePlatform: "linux",
    nodeArch: "arm64",
    name: "@dirsql/cli-linux-arm64-gnu",
    libName: "@dirsql/lib-linux-arm64-gnu",
    os: ["linux"],
    cpu: ["arm64"],
    libc: ["glibc"],
    ext: "tar.xz",
  },
  {
    triple: "x86_64-apple-darwin",
    nodePlatform: "darwin",
    nodeArch: "x64",
    name: "@dirsql/cli-darwin-x64",
    libName: "@dirsql/lib-darwin-x64",
    os: ["darwin"],
    cpu: ["x64"],
    ext: "tar.xz",
  },
  {
    triple: "aarch64-apple-darwin",
    nodePlatform: "darwin",
    nodeArch: "arm64",
    name: "@dirsql/cli-darwin-arm64",
    libName: "@dirsql/lib-darwin-arm64",
    os: ["darwin"],
    cpu: ["arm64"],
    ext: "tar.xz",
  },
  {
    triple: "x86_64-pc-windows-msvc",
    nodePlatform: "win32",
    nodeArch: "x64",
    name: "@dirsql/cli-win32-x64-msvc",
    libName: "@dirsql/lib-win32-x64-msvc",
    os: ["win32"],
    cpu: ["x64"],
    ext: "zip",
    exe: true,
  },
];

/** Node `${platform}-${arch}` → `@dirsql/cli-*` sub-package name. */
export function nodeTriples(): Record<string, string> {
  const out: Record<string, string> = {};
  for (const p of PLATFORMS) {
    out[`${p.nodePlatform}-${p.nodeArch}`] = p.name;
  }
  return out;
}

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
