// CLI e2e: the declared `[[table]] name` key through the real launcher.
//
// A table's name is declared, never derived from `ddl`. The launcher must
// query a config table under its declared `name`, exit non-zero when a
// `[[table]]` entry omits `name`, and exit non-zero when the entry's `ddl`
// never creates that name. Nothing is mocked: real launcher, real process,
// real filesystem, real SQLite, real `on-file` command spawn.

import { spawn } from "node:child_process";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

const __dirname = dirname(fileURLToPath(import.meta.url));
const PKG_ROOT = join(__dirname, "..", "..");

const PKG: { bin: { dirsql: string } } = JSON.parse(
  await readFile(join(PKG_ROOT, "package.json"), "utf8"),
);
const CLI_ENTRY = join(PKG_ROOT, PKG.bin.dirsql);

interface RunResult {
  code: number | null;
  stderr: string;
  stdout: string;
}

function runLauncher(args: string[], cwd: string): Promise<RunResult> {
  return new Promise((resolve) => {
    const proc = spawn(process.execPath, [CLI_ENTRY, ...args], {
      cwd,
      stdio: ["pipe", "pipe", "pipe"],
    });
    let stderr = "";
    let stdout = "";
    proc.stderr.setEncoding("utf8");
    proc.stdout.setEncoding("utf8");
    proc.stderr.on("data", (chunk: string) => {
      stderr += chunk;
    });
    proc.stdout.on("data", (chunk: string) => {
      stdout += chunk;
    });
    proc.stdin.end();
    proc.on("close", (code) => resolve({ code, stderr, stdout }));
    setTimeout(() => {
      if (proc.exitCode === null) {
        proc.kill("SIGKILL");
      }
    }, 20_000);
  });
}

describe("declared [[table]] name (CLI)", () => {
  let dir: string;
  let configPath: string;

  beforeEach(async () => {
    dir = await mkdtemp(join(tmpdir(), "dirsql-declared-name-e2e-"));
    configPath = join(dir, ".dirsql.toml");
    await mkdir(join(dir, "data"), { recursive: true });
    await writeFile(
      join(dir, "data", "a.json"),
      '[{"id": "one"}, {"id": "two"}]',
    );
  });

  afterEach(async () => {
    await rm(dir, { recursive: true, force: true });
  });

  it("queries a table under its declared name", async () => {
    await writeFile(
      configPath,
      `
[[table]]
name = "records"
ddl = "CREATE TABLE records (id TEXT)"
glob = "data/*.json"
on-file = "cat {path}"
`,
    );

    const { code, stdout, stderr } = await runLauncher(
      ["query", "SELECT id FROM records ORDER BY id", "--config", configPath],
      dir,
    );

    expect(code, `expected a clean run, stderr: ${stderr}`).toBe(0);
    expect((JSON.parse(stdout) as { id: string }[]).map((r) => r.id)).toEqual([
      "one",
      "two",
    ]);
  }, 30_000);

  it("exits non-zero when a [[table]] entry has no name", async () => {
    await writeFile(
      configPath,
      `
[[table]]
ddl = "CREATE TABLE records (id TEXT)"
glob = "data/*.json"
on-file = "cat {path}"
`,
    );

    const { code, stderr } = await runLauncher(
      ["query", "SELECT id FROM records", "--config", configPath],
      dir,
    );

    expect(code).not.toBe(0);
    expect(stderr).toContain("name");
    expect(stderr).toContain("[[table]]");
  }, 30_000);

  it("exits non-zero when the ddl never creates the declared name", async () => {
    await writeFile(
      configPath,
      `
[[table]]
name = "messages"
ddl = "CREATE TABLE records (id TEXT)"
glob = "data/*.json"
on-file = "cat {path}"
`,
    );

    const { code, stderr } = await runLauncher(
      ["query", "SELECT id FROM messages", "--config", configPath],
      dir,
    );

    expect(code).not.toBe(0);
    expect(stderr).toContain("table 'messages'");
  }, 30_000);
});
