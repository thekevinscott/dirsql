// Stage napi addons for every target the host runner can build, into
// `build/<triple>/` — the directory putitoutthere packages per-platform
// artifacts from (its README → Recipes → napi family).
//
// We build the whole host platform family rather than one triple (darwin host
// → both darwin-x64 and darwin-arm64; linux host → just the host triple, since
// linux-x64 and linux-arm64 each get their own native runner; windows host →
// the host triple) and let the workflow's per-row upload step take the
// directory its row wants. That is a convenience, not a necessity: the matrix
// does pass `TARGET` (and `BUILD`) per row, so a future version could build
// exactly the requested triple. Staging the extra darwin arch costs one cross
// build and keeps this script independent of the env contract.
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
import { librarySlug } from "../src/library-slug.js";
import { type Platform, PLATFORMS } from "../src/platforms.js";

export function findHostPlatform(
  key = `${process.platform}-${process.arch}`,
): Platform {
  const p = PLATFORMS.find(
    (q) => `${q.nodePlatform}-${q.nodeArch}` === key,
  );
  if (!p) {
    throw new Error(
      `unsupported host ${key}; add a row to src/platforms.json`,
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
  staged: { triple: string; napiOutDir: string }[];
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
    // `--manifest-path`/`--output-dir` point napi at the colocated
    // `dirsql-napi` crate (packages/ts/napi) while still dropping the
    // artifact into this package (packages/ts) root.
    const cross = spawn(
      "npx",
      [
        "napi",
        "build",
        "--release",
        "--platform",
        "--manifest-path",
        "napi/Cargo.toml",
        "--output-dir",
        ".",
        "--target",
        target.triple,
      ],
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

  // `build/<triple>/` is the directory the engine packages per-platform
  // artifacts from. A `napi-` prefix here matches nothing at upload time, so
  // every platform artifact ships EMPTY while the build job still reports
  // success -- publish only catches it later, at the completeness check (#788).
  // The prefix was load-bearing while npm had two build rows (napi +
  // bundled-cli) and the mode segment kept them apart; #776 removed the
  // second row, and with it the reason for the prefix.
  const napiOutDir = join(tsPkg, "build", triple);
  rmSync(napiOutDir, { recursive: true, force: true });
  mkdirSync(napiOutDir, { recursive: true });
  copyFileSync(napiSrc, join(napiOutDir, `dirsql.${triple}.node`));

  // No standalone CLI binary is staged any more (#739): the addon above
  // carries `runCli`, and the launcher calls it in-process. Building one
  // here would compile a second copy of the core that nothing ships.
  process.stdout.write(
    `staged ${triple} (${isHost ? "host" : "cross"}): napi -> ${napiOutDir}\n`,
  );
  return { triple, napiOutDir };
}

/* v8 ignore start -- script-invocation guard; tests import `stagePlatform` directly */
if (!process.env.VITEST) {
  stagePlatform();
}
/* v8 ignore stop */
