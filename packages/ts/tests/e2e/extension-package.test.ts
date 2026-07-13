// CLI e2e: load a SQLite extension referenced by **package name** through the
// real `dirsql` binary. Spawns the launcher against a real `.dirsql.toml`
// whose `[[dirsql.extension]].path` is a bare package name installed under
// `node_modules`, then queries the running HTTP server and asserts the
// extension's function is callable. No mocks.
//
// The loadable is the repo's `tests/fixtures/testext` cdylib (registers
// `dirsql_testext_answer() -> 42`), built on the fly with cargo.

import { type ChildProcess, execFileSync, spawn } from "node:child_process";
import {
  chmod,
  mkdir,
  mkdtemp,
  readFile,
  rm,
  writeFile,
} from "node:fs/promises";
import http from "node:http";
import net from "node:net";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { PLATFORMS, librarySlug } from "../../src/platforms.js";

const __dirname = dirname(fileURLToPath(import.meta.url));
const PKG_ROOT = join(__dirname, "..", "..");
const REPO_ROOT = join(PKG_ROOT, "..", "..");
const FIXTURE_MANIFEST = join(
  REPO_ROOT,
  "packages/rust/tests/fixtures/testext/Cargo.toml",
);

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

const PKG_NAME = "dirsql-testext-pkg";
const PKG_DIR = join(PKG_ROOT, "node_modules", PKG_NAME);

// The Node launcher (`dist/cli/dirsql.js`) resolves the real binary via
// `require.resolve("@dirsql/cli-<slug>/dirsql")`. That optional-dep package
// isn't installed in this dev tree, so stage the freshly-built binary as it so
// the launcher — which is the entry point under test — can find it.
const CLI_PKG_DIR = join(PKG_ROOT, "node_modules", HOST.name);
// The Node launcher entry (package.json `bin.dirsql`), set in beforeAll.
let LAUNCHER: string;

function buildFixtureExtension(targetDir: string): string {
  const stdout = execFileSync(
    process.env.CARGO ?? "cargo",
    [
      "build",
      "--manifest-path",
      FIXTURE_MANIFEST,
      "--target-dir",
      targetDir,
      "--message-format=json",
    ],
    { encoding: "utf8", maxBuffer: 64 * 1024 * 1024 },
  );
  let artifact: string | undefined;
  for (const line of stdout.split("\n")) {
    if (!line) {
      continue;
    }
    let msg: { reason?: string; filenames?: string[] };
    try {
      msg = JSON.parse(line);
    } catch {
      continue;
    }
    if (msg.reason === "compiler-artifact" && msg.filenames) {
      for (const f of msg.filenames) {
        if (f.endsWith(".so") || f.endsWith(".dylib") || f.endsWith(".dll")) {
          artifact = f;
        }
      }
    }
  }
  if (!artifact) {
    throw new Error("no cdylib artifact in cargo build output");
  }
  return artifact;
}

let SHIM_DIR: string;

beforeAll(async () => {
  // Install the fixture as a real package under the SDK's node_modules so
  // `require.resolve` (in the Node launcher) can find it by bare name.
  const buildDir = await mkdtemp(join(tmpdir(), "dirsql-ext-e2e-build-"));
  const so = buildFixtureExtension(join(buildDir, "target"));
  await mkdir(PKG_DIR, { recursive: true });
  await writeFile(
    join(PKG_DIR, "package.json"),
    JSON.stringify({ name: PKG_NAME, version: "0.0.0" }),
  );
  execFileSync("cp", [so, join(PKG_DIR, "testext.so")]);

  const pkg = JSON.parse(
    await readFile(join(PKG_ROOT, "package.json"), "utf8"),
  ) as { bin: { dirsql: string } };
  LAUNCHER = join(PKG_ROOT, pkg.bin.dirsql);

  // Stage the built binary as the `@dirsql/cli-<slug>` sub-package the launcher
  // resolves.
  const binName = HOST.exe ? "dirsql.exe" : "dirsql";
  await mkdir(CLI_PKG_DIR, { recursive: true });
  await writeFile(
    join(CLI_PKG_DIR, "package.json"),
    JSON.stringify({ name: HOST.name, version: "0.0.0" }),
  );
  execFileSync("cp", [BINARY, join(CLI_PKG_DIR, binName)]);

  // Shim so the binary's `dirsql interpret` spawn (native configs) resolves
  // to the Node launcher.
  SHIM_DIR = await mkdtemp(join(tmpdir(), "dirsql-shim-"));
  const shim = join(SHIM_DIR, "dirsql");
  await writeFile(
    shim,
    `#!/usr/bin/env bash\nexec "${process.execPath}" "${LAUNCHER}" "$@"\n`,
  );
  await chmod(shim, 0o755);
}, 120_000);

afterAll(async () => {
  await rm(PKG_DIR, { recursive: true, force: true });
  await rm(CLI_PKG_DIR, { recursive: true, force: true });
});

function freePort(): Promise<number> {
  return new Promise((resolve) => {
    const srv = net.createServer();
    srv.listen(0, "127.0.0.1", () => {
      const { port } = srv.address() as net.AddressInfo;
      srv.close(() => resolve(port));
    });
  });
}

function waitForServer(proc: ChildProcess, port: number, timeout = 5_000) {
  return new Promise<boolean>((resolve) => {
    let done = false;
    const fin = (v: boolean) => {
      if (!done) {
        done = true;
        resolve(v);
      }
    };
    setTimeout(() => fin(false), timeout);
    proc.once("exit", () => fin(false));
    (function attempt() {
      if (done) {
        return;
      }
      const sock = net.createConnection({ host: "127.0.0.1", port });
      sock.on("connect", () => {
        sock.destroy();
        fin(true);
      });
      sock.on("error", () => {
        sock.destroy();
        if (!done) {
          setTimeout(attempt, 50);
        }
      });
    })();
  });
}

function httpQuery(port: number, sql: string): Promise<unknown[]> {
  return new Promise((resolve, reject) => {
    const body = JSON.stringify({ sql });
    const req = http.request(
      {
        host: "127.0.0.1",
        port,
        path: "/query",
        method: "POST",
        headers: {
          "content-type": "application/json",
          "content-length": Buffer.byteLength(body),
        },
      },
      (res) => {
        let data = "";
        res.on("data", (chunk) => {
          data += chunk;
        });
        res.on("end", () => resolve(JSON.parse(data) as unknown[]));
      },
    );
    req.on("error", reject);
    req.write(body);
    req.end();
  });
}

async function runServerAndQuery(configPath: string): Promise<unknown[]> {
  const port = await freePort();
  // Drive the real Node launcher (not the binary directly): the launcher is
  // where TOML config extensions are resolved before the binary is spawned.
  const proc = spawn(
    process.execPath,
    [
      LAUNCHER,
      "--config",
      configPath,
      "--host",
      "127.0.0.1",
      "--port",
      String(port),
    ],
    {
      stdio: "pipe",
      // The CLI roots the index at its invocation cwd, so run from the data
      // directory the config's tables are defined against.
      cwd: dirname(configPath),
      env: { ...process.env, PATH: `${SHIM_DIR}:${process.env.PATH ?? ""}` },
    },
  );
  let stdout = "";
  let stderr = "";
  proc.stdout?.on("data", (c: Buffer) => {
    stdout += c.toString();
  });
  proc.stderr?.on("data", (c: Buffer) => {
    stderr += c.toString();
  });
  try {
    const ok = await waitForServer(proc, port);
    if (!ok) {
      throw new Error(
        `dirsql server did not start\n--- stdout ---\n${stdout}\n--- stderr ---\n${stderr}`,
      );
    }
    return await httpQuery(port, "SELECT dirsql_testext_answer() AS a");
  } finally {
    proc.kill("SIGTERM");
    await new Promise<void>((resolve) => proc.once("exit", () => resolve()));
  }
}

describe("dirsql CLI --config: extension by package name (#299)", () => {
  it("loads a TOML `[[dirsql.extension]]` referenced by package name", async () => {
    const dir = await mkdtemp(join(tmpdir(), "dirsql-ext-toml-"));
    await writeFile(join(dir, "a.txt"), "x");
    await writeFile(
      join(dir, ".dirsql.toml"),
      `[[dirsql.extension]]\npath = "${PKG_NAME}"\nentrypoint = "sqlite3_extension_init"\n\n[[table]]\nddl = "CREATE TABLE files (path TEXT)"\nglob = "*.txt"\n`,
    );
    const rows = await runServerAndQuery(join(dir, ".dirsql.toml"));
    expect(rows).toEqual([{ a: 42 }]);
  }, 30_000);
});
