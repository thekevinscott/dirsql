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
import { buildSetFor, findHostPlatform, stagePlatform } from "./stage-platform.js";
import type { Platform } from "../src/platforms.js";

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

const linuxX64: Platform = {
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

const darwinArm64: Platform = {
  triple: "aarch64-apple-darwin",
  nodePlatform: "darwin",
  nodeArch: "arm64",
  name: "@dirsql/cli-darwin-arm64",
  libName: "@dirsql/lib-darwin-arm64",
  os: ["darwin"],
  cpu: ["arm64"],
  ext: "tar.xz",
};

const windowsX64: Platform = {
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

const slugFor: Record<string, string> = {
  "x86_64-unknown-linux-gnu": "linux-x64-gnu",
  "x86_64-pc-windows-msvc": "win32-x64-msvc",
  "aarch64-apple-darwin": "darwin-arm64",
  "x86_64-apple-darwin": "darwin-x64",
};

/** Drop a .node file at the package root (mimics the host napi:build's effect). */
function seedHostNapiOutput(triple: string, mode: "suffixed" | "unsuffixed") {
  const slug = slugFor[triple] ?? "linux-x64-gnu";
  const name = mode === "suffixed" ? `dirsql.${slug}.node` : "dirsql.node";
  writeFileSync(join(tsPkg, name), "fake-host-node");
}

/** Spawn fake that handles `rustup`, `cargo`, and cross-target `napi build`. */
function fakeSpawn(opts: { binName?: string }) {
  const binName = opts.binName ?? "dirsql";
  return vi.fn((cmd: string, args: readonly string[]) => {
    if (cmd === "npx" && args[0] === "napi") {
      // Cross-target napi build: emit dirsql.<slug>.node based on --target.
      const targetIdx = args.indexOf("--target");
      const triple = targetIdx >= 0 ? args[targetIdx + 1] : "";
      const slug = slugFor[triple] ?? "linux-x64-gnu";
      writeFileSync(join(tsPkg, `dirsql.${slug}.node`), `fake-cross-${slug}`);
      return { status: 0 } as ReturnType<typeof spawnFn>;
    }
    if (cmd === "rustup") return { status: 0 } as ReturnType<typeof spawnFn>;
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

describe("findHostPlatform", () => {
  it("returns the matching PLATFORMS row for a known host", () => {
    const p = findHostPlatform("linux-x64");
    expect(p.name).toBe("@dirsql/cli-linux-x64-gnu");
  });

  it("throws on an unsupported host", () => {
    expect(() => findHostPlatform("freebsd-mips")).toThrow(/unsupported host/);
  });
});

describe("buildSetFor", () => {
  it("returns just the host triple for linux", () => {
    expect(buildSetFor(linuxX64).map((p) => p.triple)).toEqual([
      "x86_64-unknown-linux-gnu",
    ]);
  });

  it("returns both darwin triples when host is darwin", () => {
    const triples = buildSetFor(darwinArm64).map((p) => p.triple).sort();
    expect(triples).toEqual([
      "aarch64-apple-darwin",
      "x86_64-apple-darwin",
    ]);
  });

  it("returns just the host triple for windows", () => {
    expect(buildSetFor(windowsX64).map((p) => p.triple)).toEqual([
      "x86_64-pc-windows-msvc",
    ]);
  });
});

describe("stagePlatform", () => {
  it("on a linux-x64 host stages just the host target", () => {
    seedHostNapiOutput(linuxX64.triple, "suffixed");
    const spawn = fakeSpawn({});
    const result = stagePlatform({ tsPkg, repo, platform: linuxX64, spawn });

    expect(result.staged.map((s) => s.triple)).toEqual(["linux-x64-gnu"]);
    expect(existsSync(join(tsPkg, "build", "napi-linux-x64-gnu", "dirsql.linux-x64-gnu.node"))).toBe(true);
    expect(existsSync(join(tsPkg, "build", "bundled-cli-linux-x64-gnu", "dirsql"))).toBe(true);
  });

  it("on a darwin-arm64 host cross-compiles darwin-x64 from the arm64 host", () => {
    seedHostNapiOutput(darwinArm64.triple, "suffixed");
    const spawn = fakeSpawn({});
    const result = stagePlatform({ tsPkg, repo, platform: darwinArm64, spawn });

    const triples = result.staged.map((s) => s.triple).sort();
    expect(triples).toEqual(["darwin-arm64", "darwin-x64"]);

    // Host target picked up the seeded napi output.
    expect(readFileSync(join(tsPkg, "build", "napi-darwin-arm64", "dirsql.darwin-arm64.node"), "utf8")).toBe("fake-host-node");
    // Cross target invoked napi build --target.
    expect(readFileSync(join(tsPkg, "build", "napi-darwin-x64", "dirsql.darwin-x64.node"), "utf8")).toBe("fake-cross-darwin-x64");
    // Both cli binaries land at the right paths.
    expect(existsSync(join(tsPkg, "build", "bundled-cli-darwin-arm64", "dirsql"))).toBe(true);
    expect(existsSync(join(tsPkg, "build", "bundled-cli-darwin-x64", "dirsql"))).toBe(true);
  });

  it("uses dirsql.exe on Windows host", () => {
    seedHostNapiOutput(windowsX64.triple, "suffixed");
    const spawn = fakeSpawn({ binName: "dirsql.exe" });
    stagePlatform({ tsPkg, repo, platform: windowsX64, spawn });

    expect(existsSync(join(tsPkg, "build", "bundled-cli-win32-x64-msvc", "dirsql.exe"))).toBe(true);
  });

  it("falls back to dirsql.node when napi-rs emitted the unsuffixed name", () => {
    seedHostNapiOutput(linuxX64.triple, "unsuffixed");
    const spawn = fakeSpawn({});
    stagePlatform({ tsPkg, repo, platform: linuxX64, spawn });

    expect(existsSync(join(tsPkg, "build", "napi-linux-x64-gnu", "dirsql.linux-x64-gnu.node"))).toBe(true);
  });

  it("throws when no host .node file is present", () => {
    const spawn = fakeSpawn({});
    expect(() => stagePlatform({ tsPkg, repo, platform: linuxX64, spawn })).toThrow(/napi:build produced no .node file/);
  });

  it("throws when napi cross-build returns non-zero", () => {
    seedHostNapiOutput(darwinArm64.triple, "suffixed");
    const spawn = vi.fn((cmd: string, args: readonly string[]) => {
      if (cmd === "npx" && args[0] === "napi") return { status: 2 };
      if (cmd === "rustup") return { status: 0 };
      if (cmd === "cargo") {
        const targetIdx = args.indexOf("--target");
        const target = targetIdx >= 0 ? args[targetIdx + 1] : "default";
        const outDir = join(repo, "target", target ?? "default", "release");
        mkdirSync(outDir, { recursive: true });
        writeFileSync(join(outDir, "dirsql"), "x");
        return { status: 0 };
      }
      return { status: 1 };
    }) as unknown as typeof spawnFn;
    expect(() => stagePlatform({ tsPkg, repo, platform: darwinArm64, spawn })).toThrow(/napi cross-build for x86_64-apple-darwin failed/);
  });

  it("throws when napi cross-build emits no file", () => {
    seedHostNapiOutput(darwinArm64.triple, "suffixed");
    const spawn = vi.fn((cmd: string, args: readonly string[]) => {
      if (cmd === "npx" && args[0] === "napi") return { status: 0 }; // success but no file
      if (cmd === "rustup") return { status: 0 };
      if (cmd === "cargo") {
        const targetIdx = args.indexOf("--target");
        const target = targetIdx >= 0 ? args[targetIdx + 1] : "default";
        const outDir = join(repo, "target", target ?? "default", "release");
        mkdirSync(outDir, { recursive: true });
        writeFileSync(join(outDir, "dirsql"), "x");
        return { status: 0 };
      }
      return { status: 1 };
    }) as unknown as typeof spawnFn;
    expect(() => stagePlatform({ tsPkg, repo, platform: darwinArm64, spawn })).toThrow(/napi cross-build: missing/);
  });

  it("reports 'none' when a cross-build emits no file and the dir is empty", () => {
    // host darwin-arm64 → build set is [darwin-x64 (cross), darwin-arm64
    // (host)]; the cross target is staged first. With nothing seeded the
    // package dir holds no .node files, so the missing-output message hits
    // the `|| "none"` fallback in `here.join(", ") || "none"`.
    const spawn = vi.fn((cmd: string) => {
      if (cmd === "npx") return { status: 0 }; // success but writes no file
      if (cmd === "rustup") return { status: 0 };
      return { status: 1 };
    }) as unknown as typeof spawnFn;
    expect(() =>
      stagePlatform({ tsPkg, repo, platform: darwinArm64, spawn }),
    ).toThrow(/napi cross-build: missing .* \(saw: none\)/);
  });

  it("throws when cargo build returns non-zero", () => {
    seedHostNapiOutput(linuxX64.triple, "suffixed");
    const spawn = vi.fn((cmd: string) => {
      if (cmd === "rustup") return { status: 0 };
      return { status: 101 };
    }) as unknown as typeof spawnFn;
    expect(() => stagePlatform({ tsPkg, repo, platform: linuxX64, spawn })).toThrow(/cargo build failed.*exit 101/);
  });

  it("throws when rustup target add returns non-zero", () => {
    seedHostNapiOutput(linuxX64.triple, "suffixed");
    const spawn = vi.fn((cmd: string) => {
      if (cmd === "rustup") return { status: 1 };
      return { status: 0 };
    }) as unknown as typeof spawnFn;
    expect(() => stagePlatform({ tsPkg, repo, platform: linuxX64, spawn })).toThrow(/rustup target add.*failed/);
  });
});
