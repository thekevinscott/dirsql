// Stage napi addons + bundled CLI binaries for every target the host
// runner can build. Putitoutthere's matrix runs `npm run build` on a
// runner without passing `matrix.target`, so this script can't ask
// which (mode, triple) the current row wants. We instead build for
// every target in the same platform family as the host (darwin host →
// both darwin-x64 and darwin-arm64; linux host → just the host triple,
// since linux-x64 and linux-arm64 each have their own native runner;
// windows host → the host triple) and let the workflow's per-row
// upload step pick from `build/{mode}-{triple}/`.
//
// The napi:build wireit task produces `dirsql.<host-triple>.node` at
// the package root. For each additional in-family target we run
// `napi build --release --target <triple>` ourselves, because napi-rs
// hands us host-only by default.

import { spawnSync } from "node:child_process";
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  readdirSync,
  rmSync,
} from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { type Platform, PLATFORMS, librarySlug } from "../ts/platforms.js";

export function findHostPlatform(
  key = `${process.platform}-${process.arch}`,
): Platform {
  const p = PLATFORMS.find(
    (q) => `${q.nodePlatform}-${q.nodeArch}` === key,
  );
  if (!p) {
    throw new Error(
      `unsupported host ${key}; add a row to PLATFORMS in ts/platforms.ts`,
    );
  }
  return p;
}

/**
 * Targets we should attempt to build from `host`. Always includes the
 * host triple itself; on darwin we also include the other darwin arch
 * because Apple's toolchain ships both. Linux + Windows hosts only
 * build their own triple — within those families putitoutthere assigns
 * separate native runners per target.
 */
export function buildSetFor(host: Platform): readonly Platform[] {
  if (host.nodePlatform === "darwin") {
    return PLATFORMS.filter((p) => p.nodePlatform === "darwin");
  }
  return [host];
}

export interface StagePlatformOptions {
  /** Package directory (defaults to `packages/ts`). */
  tsPkg?: string;
  /** Workspace root (defaults to repo root, two levels up from tsPkg). */
  repo?: string;
  /** Host platform (defaults to detection from `process.platform`/`arch`). */
  platform?: Platform;
  /** Spawner; injected for tests. */
  spawn?: typeof spawnSync;
}

export interface StageResult {
  staged: { triple: string; napiOutDir: string; cliOutDir: string }[];
}

export function stagePlatform(opts: StagePlatformOptions = {}): StageResult {
  /* v8 ignore start -- trivial defaults; tests inject all four explicitly */
  const tsPkg =
    opts.tsPkg ?? resolve(fileURLToPath(import.meta.url), "..", "..");
  const repo = opts.repo ?? resolve(tsPkg, "..", "..");
  const host = opts.platform ?? findHostPlatform();
  const spawn = opts.spawn ?? spawnSync;
  /* v8 ignore stop */

  const targets = buildSetFor(host);
  const staged: StageResult["staged"] = [];

  for (const target of targets) {
    staged.push(stageOne({ tsPkg, repo, host, target, spawn }));
  }

  return { staged };
}

interface StageOneArgs {
  tsPkg: string;
  repo: string;
  host: Platform;
  target: Platform;
  spawn: typeof spawnSync;
}

function stageOne(args: StageOneArgs): StageResult["staged"][number] {
  const { tsPkg, repo, host, target, spawn } = args;
  const triple = librarySlug(target);
  const isHost = host.triple === target.triple;
  const exe = target.exe === true;
  const binName = exe ? "dirsql.exe" : "dirsql";

  // 1. Ensure the cargo target is installed (idempotent). Must run
  //    before any cross-target build — both `napi build --target` and
  //    `cargo build --target` shell out to rustc, which fails with
  //    `can't find crate for core` when the target's stdlib isn't
  //    on disk.
  const rustupAdd = spawn("rustup", ["target", "add", target.triple], {
    stdio: "inherit",
  });
  if (rustupAdd.status !== 0) {
    throw new Error(
      `rustup target add ${target.triple} failed (exit ${rustupAdd.status})`,
    );
  }

  // 2. napi binary. For the host target, napi:build (wireit dep) has
  //    already dropped `dirsql.<triple>.node` at the package root.
  //    Cross-targets (darwin-x64 from arm64, etc.) need their own
  //    `napi build --release --target <triple>` invocation.
  let napiSrc: string;
  if (isHost) {
    const suffixed = join(tsPkg, `dirsql.${triple}.node`);
    const unsuffixed = join(tsPkg, "dirsql.node");
    if (existsSync(suffixed)) napiSrc = suffixed;
    else if (existsSync(unsuffixed)) napiSrc = unsuffixed;
    else {
      const here = readdirSync(tsPkg).filter((f) => f.endsWith(".node"));
      throw new Error(
        `napi:build produced no .node file at ${suffixed} or ${unsuffixed} (saw: ${here.join(", ") || "none"}).`,
      );
    }
  } else {
    // `--platform` makes napi-rs emit `dirsql.<slug>.node` (suffixed)
    // instead of `dirsql.node`. Without it the cross-build's output
    // collides with the host's napi:build file at the package root.
    const cross = spawn(
      "npx",
      ["napi", "build", "--release", "--platform", "--target", target.triple],
      { cwd: tsPkg, stdio: "inherit" },
    );
    if (cross.status !== 0) {
      throw new Error(
        `napi cross-build for ${target.triple} failed (exit ${cross.status})`,
      );
    }
    const out = join(tsPkg, `dirsql.${triple}.node`);
    if (!existsSync(out)) {
      const here = readdirSync(tsPkg).filter((f) => f.endsWith(".node"));
      throw new Error(
        `napi cross-build: missing ${out} (saw: ${here.join(", ") || "none"})`,
      );
    }
    napiSrc = out;
  }

  const napiOutDir = join(tsPkg, "build", `napi-${triple}`);
  rmSync(napiOutDir, { recursive: true, force: true });
  mkdirSync(napiOutDir, { recursive: true });
  copyFileSync(napiSrc, join(napiOutDir, `dirsql.${triple}.node`));

  // 3. Cargo build the standalone CLI binary. The bin is gated behind
  //    `--features cli` (packages/rust/Cargo.toml `[[bin]]
  //    required-features`); without the flag cargo silently skips it.
  const cargo = spawn(
    "cargo",
    [
      "build",
      "--release",
      "--bin",
      "dirsql",
      "--features",
      "cli",
      "--manifest-path",
      join(repo, "packages", "rust", "Cargo.toml"),
      "--target",
      target.triple,
    ],
    { stdio: "inherit" },
  );
  if (cargo.status !== 0) {
    throw new Error(`cargo build failed (exit ${cargo.status})`);
  }

  const cliSrc = join(repo, "target", target.triple, "release", binName);
  /* v8 ignore start -- defensive: cargo returned 0 but produced no binary */
  if (!existsSync(cliSrc)) {
    throw new Error(`cargo build: missing binary at ${cliSrc}`);
  }
  /* v8 ignore stop */

  const cliOutDir = join(tsPkg, "build", `bundled-cli-${triple}`);
  rmSync(cliOutDir, { recursive: true, force: true });
  mkdirSync(cliOutDir, { recursive: true });
  copyFileSync(cliSrc, join(cliOutDir, binName));

  process.stdout.write(
    `staged ${triple} (${isHost ? "host" : "cross"}): napi -> ${napiOutDir}, cli -> ${cliOutDir}\n`,
  );
  return { triple, napiOutDir, cliOutDir };
}

/* v8 ignore start -- script-invocation guard; tests import `stagePlatform` directly */
if (!process.env.VITEST) {
  stagePlatform();
}
/* v8 ignore stop */
