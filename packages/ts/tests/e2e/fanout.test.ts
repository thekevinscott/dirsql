// CLI e2e: fan-out file->table matching through the real Node launcher +
// binary. A file matching two tables' overlapping globs populates both tables;
// querying each over the real `POST /query` surface returns the file's row
// (#580). No mocks: real launcher, real binary, real process, real filesystem.
//
// The binary is staged under a PRIVATE temp `node_modules` handed to the
// launcher via NODE_PATH (honored by the launcher's CJS `require.resolve`),
// rather than the package's own `node_modules` (which other e2e files
// stage/remove and vitest runs in parallel).

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
  const pkg = JSON.parse(
    await readFile(join(PKG_ROOT, "package.json"), "utf8"),
  ) as { bin: { dirsql: string } };
  LAUNCHER = join(PKG_ROOT, pkg.bin.dirsql);

  STAGE_ROOT = await mkdtemp(join(tmpdir(), "dirsql-fanout-e2e-"));
  NODE_PATH_DIR = join(STAGE_ROOT, "node_modules");
  const cliPkgDir = join(NODE_PATH_DIR, HOST.name);
  const binName = HOST.exe ? "dirsql.exe" : "dirsql";
  await mkdir(cliPkgDir, { recursive: true });
  await writeFile(
    join(cliPkgDir, "package.json"),
    JSON.stringify({ name: HOST.name, version: "0.0.0" }),
  );
  execFileSync("cp", [BINARY, join(cliPkgDir, binName)]);

  const dataDir = join(STAGE_ROOT, "data");
  await mkdir(join(dataDir, "data", "2401.00001"), { recursive: true });
  await writeFile(join(dataDir, "data", "2401.00001", "metadata.json"), "{}");
  const cfg = join(dataDir, ".dirsql.toml");
  // Each table emits its own `path` column via an `on-file` hook (the core no
  // longer injects filesystem facts): strip the root prefix and print it.
  const pathHook = `on-file = '''sh -c 'rel=\${1#"$2"/}; printf "[{\\"path\\":\\"%s\\"}]" "$rel"' sh {path} {root}'''`;
  await writeFile(
    cfg,
    `[[table]]\nddl = "CREATE TABLE ta (path TEXT)"\nglob = "data/*/metadata.json"\n${pathHook}\n\n[[table]]\nddl = "CREATE TABLE tb (path TEXT)"\nglob = "data/**/metadata.json"\n${pathHook}\n`,
  );

  serverPort = await freePort();
  serverProc = spawn(
    process.execPath,
    [
      LAUNCHER,
      "server",
      "--config",
      cfg,
      "--host",
      "127.0.0.1",
      "--port",
      String(serverPort),
    ],
    {
      stdio: "pipe",
      cwd: dataDir,
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

describe("fan-out file->table matching (#580)", () => {
  it("populates the first-declared table", async () => {
    const res = await postSql(serverPort, "SELECT path FROM ta");
    expect(res.status).toBe(200);
    const rows = JSON.parse(res.text) as { path: string }[];
    expect(rows.map((r) => r.path)).toEqual(["data/2401.00001/metadata.json"]);
  });

  it("also populates the second-declared table", async () => {
    const res = await postSql(serverPort, "SELECT path FROM tb");
    expect(res.status).toBe(200);
    const rows = JSON.parse(res.text) as { path: string }[];
    expect(rows.map((r) => r.path)).toEqual(["data/2401.00001/metadata.json"]);
  });
});
