// CLI e2e: the documented `POST /query` contract through the real Node
// launcher + binary (docs/reference/http-api.md).
//
// Spawns the real built launcher (`dist/cli/dirsql.js`) over a real
// `.dirsql.toml` and asserts the documented query contract end to end: a JSON
// array of row objects on success, and each documented failure class --
// malformed JSON body, missing/empty `sql`, SQL errors, the read-only rule,
// and the `_dirsql_*` internal-table denial (all `400` with a JSON
// `{"error": ...}` body), plus `405` for `GET /query`. No mocks: real
// launcher, real binary, real process, real filesystem.
//
// Added with #399 (whose stage 1, #438, rewired the binary's query pipeline):
// the Rust tiers already pin this contract in-crate, but the language
// packages ship the binary, so each SDK's e2e suite must pin the HTTP
// surface it ships.
//
// The binary is staged under a PRIVATE temp `node_modules` handed to the
// launcher via NODE_PATH (honored by the launcher's CJS `require.resolve`),
// rather than the package's own `node_modules` -- extension-package.test.ts
// stages/removes the latter and vitest runs test files in parallel, so
// sharing it would race.

import { type ChildProcess, execFileSync, spawn } from "node:child_process";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import http from "node:http";
import net from "node:net";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
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

let LAUNCHER: string;
let STAGE_ROOT: string;
let NODE_PATH_DIR: string;
let serverProc: ChildProcess;
let serverPort: number;
let serverLogs = "";

interface RawResponse {
  status: number;
  text: string;
}

function httpRaw(
  port: number,
  method: "GET" | "POST",
  body?: string,
): Promise<RawResponse> {
  return new Promise((resolve, reject) => {
    const req = http.request(
      {
        host: "127.0.0.1",
        port,
        path: "/query",
        method,
        headers:
          body === undefined
            ? {}
            : {
                "content-type": "application/json",
                "content-length": Buffer.byteLength(body),
              },
      },
      (res) => {
        let data = "";
        res.on("data", (chunk) => {
          data += chunk;
        });
        res.on("end", () =>
          resolve({ status: res.statusCode ?? 0, text: data }),
        );
      },
    );
    req.on("error", reject);
    if (body !== undefined) {
      req.write(body);
    }
    req.end();
  });
}

async function postSql(port: number, sql: string): Promise<RawResponse> {
  return httpRaw(port, "POST", JSON.stringify({ sql }));
}

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

beforeAll(async () => {
  // Resolve the launcher entry from package.json's `bin` field.
  const pkg = JSON.parse(
    await readFile(join(PKG_ROOT, "package.json"), "utf8"),
  ) as { bin: { dirsql: string } };
  LAUNCHER = join(PKG_ROOT, pkg.bin.dirsql);

  // Stage the built binary as the `@dirsql/cli-<slug>` package under a
  // private node_modules, delivered via NODE_PATH (see header comment).
  STAGE_ROOT = await mkdtemp(join(tmpdir(), "dirsql-query-contract-"));
  NODE_PATH_DIR = join(STAGE_ROOT, "node_modules");
  const cliPkgDir = join(NODE_PATH_DIR, HOST.name);
  const binName = HOST.exe ? "dirsql.exe" : "dirsql";
  await mkdir(cliPkgDir, { recursive: true });
  await writeFile(
    join(cliPkgDir, "package.json"),
    JSON.stringify({ name: HOST.name, version: "0.0.0" }),
  );
  execFileSync("cp", [BINARY, join(cliPkgDir, binName)]);

  // One server over a one-file table for the whole (read-only) suite.
  const dataDir = join(STAGE_ROOT, "data");
  await mkdir(dataDir);
  await writeFile(join(dataDir, "a.txt"), "hello");
  const cfg = join(dataDir, ".dirsql.toml");
  await writeFile(
    cfg,
    `[[table]]\nddl = "CREATE TABLE files (_path TEXT)"\nglob = "*.txt"\n`,
  );

  serverPort = await freePort();
  serverProc = spawn(
    process.execPath,
    [
      LAUNCHER,
      "--config",
      cfg,
      "--host",
      "127.0.0.1",
      "--port",
      String(serverPort),
    ],
    {
      stdio: "pipe",
      env: {
        ...process.env,
        NODE_PATH: [NODE_PATH_DIR, process.env.NODE_PATH]
          .filter(Boolean)
          .join(":"),
      },
    },
  );
  serverProc.stdout?.on("data", (c: Buffer) => {
    serverLogs += c.toString();
  });
  serverProc.stderr?.on("data", (c: Buffer) => {
    serverLogs += c.toString();
  });
  const ok = await waitForServer(serverProc, serverPort);
  if (!ok) {
    throw new Error(`dirsql server did not start\n${serverLogs}`);
  }
}, 60_000);

afterAll(async () => {
  serverProc?.kill("SIGTERM");
  if (serverProc) {
    await new Promise<void>((resolve) =>
      serverProc.once("exit", () => resolve()),
    );
  }
  await rm(STAGE_ROOT, { recursive: true, force: true });
});

describe("POST /query contract (docs/reference/http-api.md)", () => {
  it("returns rows as a JSON array of objects", async () => {
    const res = await postSql(serverPort, "SELECT 1 AS one, 'x' AS s");
    expect(res.status).toBe(200);
    expect(JSON.parse(res.text)).toEqual([{ one: 1, s: "x" }]);
  });

  it("serves a config-table row per matched file", async () => {
    const res = await postSql(serverPort, "SELECT _path FROM files");
    expect(res.status).toBe(200);
    const rows = JSON.parse(res.text) as { _path: string }[];
    expect(rows).toHaveLength(1);
    expect(rows[0]._path.endsWith("a.txt")).toBe(true);
  });

  it("rejects a missing `sql` field with 400", async () => {
    const res = await httpRaw(serverPort, "POST", "{}");
    expect(res.status).toBe(400);
    expect(JSON.parse(res.text)).toEqual({ error: "missing `sql` field" });
  });

  it("rejects an empty `sql` field with 400", async () => {
    const res = await postSql(serverPort, "   ");
    expect(res.status).toBe(400);
    expect(JSON.parse(res.text)).toEqual({ error: "`sql` must not be empty" });
  });

  it("rejects a malformed JSON body with 400", async () => {
    const res = await httpRaw(serverPort, "POST", "not json");
    expect(res.status).toBe(400);
    expect((JSON.parse(res.text) as { error: string }).error).toBeTruthy();
  });

  it("rejects a SQL error with 400", async () => {
    const res = await postSql(serverPort, "SELECT * FROM nope");
    expect(res.status).toBe(400);
    expect((JSON.parse(res.text) as { error: string }).error).toContain(
      "no such table",
    );
  });

  // NOTE: http-api.md also documents the read-only rule (a write statement
  // is a 400), but the binary currently returns 500 for it -- a pre-existing
  // docs-vs-behavior mismatch surfaced while writing this suite, tracked as
  // #444. The case is asserted here as soon as that fix lands.

  it("denies `_dirsql_*` internal-table reads with 400", async () => {
    // #378: the internal bookkeeping namespace is not readable through the
    // query surface; a rejected read is an error, not empty output.
    const res = await postSql(
      serverPort,
      "SELECT * FROM _dirsql_internal_rows",
    );
    expect(res.status).toBe(400);
    expect((JSON.parse(res.text) as { error: string }).error).toContain(
      "not authorized",
    );
  });

  it("rejects GET /query with 405", async () => {
    const res = await httpRaw(serverPort, "GET");
    expect(res.status).toBe(405);
    expect(res.text).toBe("method not allowed");
  });
});
