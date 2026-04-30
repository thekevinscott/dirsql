import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { findHostPlatform, stagePlatform } from "./stagePlatform.js";
import type { Platform } from "../ts/platforms.js";

let work: string;
let tsPkg: string;
let repo: string;

beforeEach(() => {
  work = mkdtempSync(join(tmpdir(), "stagePlatform-"));
  tsPkg = join(work, "packages", "ts");
  repo = work;
  mkdirSync(tsPkg, { recursive: true });
});

afterEach(() => {
  rmSync(work, { recursive: true, force: true });
});

const linux: Platform = {
  triple: "x86_64-unknown-linux-gnu",
  nodePlatform: "linux",
  nodeArch: "x64",
  name: "@dirsql/cli-linux-x64-gnu",
  libName: "@dirsql/lib-linux-x64-gnu",
  os: ["linux"],
  cpu: ["x64"],
  libc: ["glibc"],
  ext: "tar.xz",
};

const windows: Platform = {
  triple: "x86_64-pc-windows-msvc",
  nodePlatform: "win32",
  nodeArch: "x64",
  name: "@dirsql/cli-win32-x64-msvc",
  libName: "@dirsql/lib-win32-x64-msvc",
  os: ["win32"],
  cpu: ["x64"],
  ext: "zip",
  exe: true,
};

/** Spawn fake that drops a `.node` file for `napi build` and a binary
 *  under `target/<triple>/release/` for `cargo build`, then returns
 *  status 0. */
function fakeSpawn(opts: {
  binName?: string;
  triple: string;
  napiSuffix?: "suffixed" | "unsuffixed" | "missing";
}) {
  const napiSuffix = opts.napiSuffix ?? "suffixed";
  const binName = opts.binName ?? "dirsql";
  return vi.fn((cmd: string, args: readonly string[]) => {
    if (cmd === "npx" && args[0] === "napi") {
      if (napiSuffix === "suffixed") {
        writeFileSync(
          join(tsPkg, `dirsql.${librarySlugFor(opts.triple)}.node`),
          "fake-node",
        );
      } else if (napiSuffix === "unsuffixed") {
        writeFileSync(join(tsPkg, "dirsql.node"), "fake-node");
      }
      // "missing": don't write anything
      return { status: 0 } as ReturnType<typeof spawnFn>;
    }
    if (cmd === "rustup") {
      return { status: 0 } as ReturnType<typeof spawnFn>;
    }
    if (cmd === "cargo") {
      const targetIdx = args.indexOf("--target");
      const target = targetIdx >= 0 ? args[targetIdx + 1] : "default";
      const outDir = join(repo, "target", target ?? "default", "release");
      mkdirSync(outDir, { recursive: true });
      writeFileSync(join(outDir, binName), "fake-bin");
      return { status: 0 } as ReturnType<typeof spawnFn>;
    }
    return { status: 1 } as ReturnType<typeof spawnFn>;
  }) as unknown as typeof spawnFn;
}
type spawnFn = (
  cmd: string,
  args: readonly string[],
  opts?: unknown,
) => { status: number | null };

function librarySlugFor(triple: string): string {
  const slugMap: Record<string, string> = {
    "x86_64-unknown-linux-gnu": "linux-x64-gnu",
    "x86_64-pc-windows-msvc": "win32-x64-msvc",
    "aarch64-apple-darwin": "darwin-arm64",
  };
  const s = slugMap[triple];
  if (!s) throw new Error(`no slug for ${triple} in test fixture`);
  return s;
}

describe("findHostPlatform", () => {
  it("returns the matching PLATFORMS row for a known host", () => {
    const p = findHostPlatform("linux-x64");
    expect(p.name).toBe("@dirsql/cli-linux-x64-gnu");
  });

  it("throws on an unsupported host", () => {
    expect(() => findHostPlatform("freebsd-mips")).toThrow(/unsupported host/);
  });
});

describe("stagePlatform", () => {
  it("stages both napi and cli outputs for a Linux host", () => {
    const spawn = fakeSpawn({ triple: linux.triple });
    const result = stagePlatform({ tsPkg, repo, platform: linux, spawn });

    expect(result.triple).toBe("linux-x64-gnu");
    expect(existsSync(join(tsPkg, "build", "napi-linux-x64-gnu", "dirsql.linux-x64-gnu.node"))).toBe(true);
    expect(existsSync(join(tsPkg, "build", "bundled-cli-linux-x64-gnu", "dirsql"))).toBe(true);
  });

  it("uses dirsql.exe on Windows", () => {
    const spawn = fakeSpawn({ binName: "dirsql.exe", triple: windows.triple });
    stagePlatform({ tsPkg, repo, platform: windows, spawn });

    expect(existsSync(join(tsPkg, "build", "bundled-cli-win32-x64-msvc", "dirsql.exe"))).toBe(true);
  });

  it("falls back to dirsql.node when napi-rs emits the unsuffixed name", () => {
    const spawn = fakeSpawn({ triple: linux.triple, napiSuffix: "unsuffixed" });
    stagePlatform({ tsPkg, repo, platform: linux, spawn });

    expect(existsSync(join(tsPkg, "build", "napi-linux-x64-gnu", "dirsql.linux-x64-gnu.node"))).toBe(true);
  });

  it("throws when napi build produces no .node file", () => {
    const spawn = fakeSpawn({ triple: linux.triple, napiSuffix: "missing" });
    expect(() => stagePlatform({ tsPkg, repo, platform: linux, spawn })).toThrow(/napi build produced no .node file/);
  });

  it("throws when napi build returns non-zero", () => {
    const spawn = vi.fn((cmd: string) => {
      if (cmd === "npx") return { status: 2 };
      return { status: 0 };
    }) as unknown as typeof spawnFn;
    expect(() => stagePlatform({ tsPkg, repo, platform: linux, spawn })).toThrow(/napi build failed.*exit 2/);
  });

  it("throws when cargo build returns non-zero", () => {
    const spawn = vi.fn((cmd: string) => {
      if (cmd === "npx") {
        writeFileSync(join(tsPkg, "dirsql.node"), "fake-node");
        return { status: 0 };
      }
      if (cmd === "rustup") return { status: 0 };
      return { status: 101 };
    }) as unknown as typeof spawnFn;
    expect(() => stagePlatform({ tsPkg, repo, platform: linux, spawn })).toThrow(/cargo build failed.*exit 101/);
  });

  it("throws when rustup target add returns non-zero", () => {
    const spawn = vi.fn((cmd: string) => {
      if (cmd === "npx") {
        writeFileSync(join(tsPkg, "dirsql.node"), "fake-node");
        return { status: 0 };
      }
      if (cmd === "rustup") return { status: 1 };
      return { status: 0 };
    }) as unknown as typeof spawnFn;
    expect(() => stagePlatform({ tsPkg, repo, platform: linux, spawn })).toThrow(/rustup target add.*failed/);
  });

  it("overwrites existing build output (idempotent re-run)", () => {
    const stale = join(tsPkg, "build", "napi-linux-x64-gnu", "stale.node");
    mkdirSync(join(tsPkg, "build", "napi-linux-x64-gnu"), { recursive: true });
    writeFileSync(stale, "stale");

    const spawn = fakeSpawn({ triple: linux.triple });
    stagePlatform({ tsPkg, repo, platform: linux, spawn });

    expect(existsSync(stale)).toBe(false);
    expect(
      readFileSync(
        join(tsPkg, "build", "napi-linux-x64-gnu", "dirsql.linux-x64-gnu.node"),
        "utf8",
      ),
    ).toBe("fake-node");
  });
});
