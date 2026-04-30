// Stage napi addon + bundled CLI binary for the host triple under
// `build/{mode}-{triple}/`. Putitoutthere's reusable workflow runs each
// (mode, triple) matrix row on a runner native to its target, so we only
// ever need to produce host-target binaries — no cross-compile.
//
// The reusable workflow does NOT pass `matrix.target` / `matrix.build`
// to `npm run build`, so this script can't tell whether the current row
// wants napi or bundled-cli. We produce both every run; the workflow's
// per-row upload step (`actions/upload-artifact@v7` with
// `path: build/{mode}-{triple}/`) picks only the matching subdir.
// Building both is wasteful by 2× per target, but it's the simplest
// correct contract until putitoutthere surfaces matrix vars.

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
  triple: string;
  napiOutDir: string;
  cliOutDir: string;
}

export function stagePlatform(opts: StagePlatformOptions = {}): StageResult {
  /* v8 ignore start -- trivial defaults; tests inject all four explicitly */
  const tsPkg =
    opts.tsPkg ?? resolve(fileURLToPath(import.meta.url), "..", "..");
  const repo = opts.repo ?? resolve(tsPkg, "..", "..");
  const platform = opts.platform ?? findHostPlatform();
  const spawn = opts.spawn ?? spawnSync;
  /* v8 ignore stop */
  const triple = librarySlug(platform);
  const exe = platform.exe === true;
  const binName = exe ? "dirsql.exe" : "dirsql";

  // 1. Pick up napi-rs's output. The `napi:build` wireit task is a
  //    dependency of `stage:platform`, so the .node file is already on
  //    disk. napi-rs CLI v3 emits `dirsql.<slug>.node` for native
  //    builds; older versions sometimes drop `dirsql.node` (unsuffixed).
  //    Probe both.
  const napiSuffixed = join(tsPkg, `dirsql.${triple}.node`);
  const napiUnsuffixed = join(tsPkg, "dirsql.node");
  let napiSrc: string;
  if (existsSync(napiSuffixed)) napiSrc = napiSuffixed;
  else if (existsSync(napiUnsuffixed)) napiSrc = napiUnsuffixed;
  else {
    const here = readdirSync(tsPkg).filter((f) => f.endsWith(".node"));
    throw new Error(
      `napi:build produced no .node file at ${napiSuffixed} or ${napiUnsuffixed} (saw: ${here.join(", ") || "none"}). The napi:build wireit task should run before stage:platform.`,
    );
  }

  const napiOutDir = join(tsPkg, "build", `napi-${triple}`);
  rmSync(napiOutDir, { recursive: true, force: true });
  mkdirSync(napiOutDir, { recursive: true });
  copyFileSync(napiSrc, join(napiOutDir, `dirsql.${triple}.node`));

  // 2. Ensure the cargo target is installed. GHA's macos / windows
  //    runners pre-install rustup with only the host triple; the
  //    `--target <triple>` form below otherwise errors with `the target
  //    may not be installed`. `rustup target add` is idempotent.
  const rustupAdd = spawn("rustup", ["target", "add", platform.triple], {
    stdio: "inherit",
  });
  if (rustupAdd.status !== 0) {
    throw new Error(`rustup target add ${platform.triple} failed (exit ${rustupAdd.status})`);
  }

  // 3. Cargo build the standalone CLI binary. The bin is gated behind
  //    `--features cli` (see packages/rust/Cargo.toml `[[bin]]
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
      platform.triple,
    ],
    { stdio: "inherit" },
  );
  if (cargo.status !== 0) {
    throw new Error(`cargo build failed (exit ${cargo.status})`);
  }

  const cliSrc = join(repo, "target", platform.triple, "release", binName);
  /* v8 ignore start -- defensive: cargo returned 0 but produced no binary */
  if (!existsSync(cliSrc)) {
    throw new Error(`cargo build: missing binary at ${cliSrc}`);
  }
  /* v8 ignore stop */

  const cliOutDir = join(tsPkg, "build", `bundled-cli-${triple}`);
  rmSync(cliOutDir, { recursive: true, force: true });
  mkdirSync(cliOutDir, { recursive: true });
  copyFileSync(cliSrc, join(cliOutDir, binName));

  process.stdout.write(`staged ${triple}: napi -> ${napiOutDir}, cli -> ${cliOutDir}\n`);
  return { triple, napiOutDir, cliOutDir };
}

/* v8 ignore start -- script-invocation guard; tests import `stagePlatform` directly */
if (!process.env.VITEST) {
  stagePlatform();
}
/* v8 ignore stop */
