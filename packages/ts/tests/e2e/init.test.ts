// CLI e2e: `dirsql init` writes a fixed starter `.dirsql.toml` -- the same
// single `files` table zero-config mode serves -- regardless of the target
// directory's contents. No LLM, no network, no filesystem inspection.
// No mocks: spawns the real built CLI binary against a real filesystem.

import { type SpawnSyncReturns, spawnSync } from "node:child_process";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { PLATFORMS, librarySlug } from "../../src/platforms.js";

const __dirname = dirname(fileURLToPath(import.meta.url));
const PKG_ROOT = join(__dirname, "..", "..");

const HOST = PLATFORMS.find(
  (p) =>
    `${p.nodePlatform}-${p.nodeArch}` === `${process.platform}-${process.arch}`,
);
if (!HOST) {
  throw new Error(`unsupported host ${process.platform}-${process.arch}`);
}
const BINARY = join(
  PKG_ROOT,
  "build",
  `bundled-cli-${librarySlug(HOST)}`,
  HOST.exe ? "dirsql.exe" : "dirsql",
);

// The exact, fixed starter config `init` writes -- byte-for-byte the same
// `[[table]]` block the zero-config default (`default_files_table` in
// `packages/rust/src/bin/dirsql.rs`) uses.
const EXPECTED_TOML =
  '[[table]]\nddl  = "CREATE TABLE files (_path TEXT, _basename TEXT, _dir TEXT, _ext TEXT, _size INTEGER, _mtime INTEGER, _ctime INTEGER)"\nglob = "**/*"\n';

let cwd: string;

beforeEach(async () => {
  cwd = await mkdtemp(join(tmpdir(), "dirsql-init-e2e-"));
});

afterEach(async () => {
  await rm(cwd, { recursive: true, force: true });
});

function runInit(args: string[] = [], runCwd = cwd): SpawnSyncReturns<string> {
  return spawnSync(BINARY, ["init", ...args], {
    cwd: runCwd,
    stdio: ["ignore", "pipe", "pipe"],
    encoding: "utf8",
  });
}

describe("dirsql init", () => {
  it("writes the fixed default files-table config", async () => {
    const result = runInit();
    expect(result.status, `stderr: ${result.stderr}`).toBe(0);

    const toml = await readFile(join(cwd, ".dirsql.toml"), "utf8");
    expect(toml).toBe(EXPECTED_TOML);
  });

  it("produces the same output regardless of directory contents", async () => {
    await writeFile(join(cwd, "notes.txt"), "hello");
    await writeFile(join(cwd, "data.json"), '{"a": 1}');
    await mkdir(join(cwd, "nested"));
    await writeFile(join(cwd, "nested", "a.md"), "hi");

    const result = runInit();
    expect(result.status, `stderr: ${result.stderr}`).toBe(0);

    const toml = await readFile(join(cwd, ".dirsql.toml"), "utf8");
    expect(toml).toBe(EXPECTED_TOML);
  });

  it("refuses to overwrite an existing config", async () => {
    await writeFile(join(cwd, ".dirsql.toml"), "# old\n");

    const result = runInit();
    expect(result.status).not.toBe(0);

    const toml = await readFile(join(cwd, ".dirsql.toml"), "utf8");
    expect(toml).toBe("# old\n");
  });

  it("overwrites the existing config with --force", async () => {
    await writeFile(join(cwd, ".dirsql.toml"), "# old\n");

    const result = runInit(["--force"]);
    expect(result.status, `stderr: ${result.stderr}`).toBe(0);

    const toml = await readFile(join(cwd, ".dirsql.toml"), "utf8");
    expect(toml).toBe(EXPECTED_TOML);
  });
});
