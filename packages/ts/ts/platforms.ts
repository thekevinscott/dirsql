// Single source of truth for the target platforms `dirsql` publishes.
//
// Indexed two ways:
//
// 1. `PLATFORMS` — the canonical list, keyed by Rust target triple. The
//    release pipeline iterates this list: `dist` builds an archive per
//    triple, and `tools/buildPlatforms.ts` synthesizes one
//    `@dirsql/cli-<name>` sub-package from each archive.
// 2. `nodeTriples()` — a lookup from `${process.platform}-${process.arch}`
//    (the identifiers Node.js exposes at runtime) to the sub-package
//    name. The bin launcher consumes this to find the matching binary
//    via `require.resolve`.

export interface Platform {
  /** Rust target triple — the name cargo-dist uses for archives. */
  triple: string;
  /** Node `process.platform` value for this target. */
  nodePlatform: NodeJS.Platform;
  /** Node `process.arch` value for this target. */
  nodeArch: NodeJS.Architecture;
  /** Published npm sub-package name (`@dirsql/cli-<slug>`). */
  name: string;
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
    os: ["darwin"],
    cpu: ["x64"],
    ext: "tar.xz",
  },
  {
    triple: "aarch64-apple-darwin",
    nodePlatform: "darwin",
    nodeArch: "arm64",
    name: "@dirsql/cli-darwin-arm64",
    os: ["darwin"],
    cpu: ["arm64"],
    ext: "tar.xz",
  },
  {
    triple: "x86_64-pc-windows-msvc",
    nodePlatform: "win32",
    nodeArch: "x64",
    name: "@dirsql/cli-win32-x64-msvc",
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
