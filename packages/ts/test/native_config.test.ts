// CLI integration test for native JavaScript config file support.
//
// Spawns the bundled Rust binary directly against the
// `__fixtures__/dirsql.config.mjs` fixture and asserts the HTTP server
// serves the `papers` table. Bypasses the TS launcher because the
// architectural property under test ("the Rust binary dispatches
// non-TOML configs to `dirsql interpret`") is the binary's job — the
// launcher is a transparent forwarder already covered by the
// `cli_smoke` e2e.

import { type ChildProcess, spawn } from "node:child_process";
import { chmod, mkdtemp, readFile, writeFile } from "node:fs/promises";
import http from "node:http";
import net from "node:net";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { beforeAll, describe, expect, it } from "vitest";
import { PLATFORMS, librarySlug } from "../src/platforms.js";

const __dirname = dirname(fileURLToPath(import.meta.url));
const PKG_ROOT = join(__dirname, "..");

// Bundled Rust binary, staged by `tools/stagePlatform.ts` under
// `build/bundled-cli-<slug>/`. `pnpm build` is a dependency of the
// `test:integration` wireit task.
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

const CONFIG_DIR = join(__dirname, "__fixtures__");

// The Rust binary's native-config dispatcher spawns `dirsql interpret <X>`
// via PATH. In a real `npm install -g dirsql` that resolves to the Node
// launcher; in this dev tree there's no global `dirsql` on PATH yet, so
// drop a shim that re-invokes the Node launcher into a tempdir and
// prepend it to PATH before spawning the binary. The CLI entry is read from
// package.json's `bin` field inside the hook (it's only needed there).
let SHIM_DIR: string;
beforeAll(async () => {
  const pkg = JSON.parse(
    await readFile(join(PKG_ROOT, "package.json"), "utf8"),
  ) as { bin: { dirsql: string } };
  const cliEntry = join(PKG_ROOT, pkg.bin.dirsql);
  SHIM_DIR = await mkdtemp(join(tmpdir(), "dirsql-shim-"));
  const shim = join(SHIM_DIR, "dirsql");
  await writeFile(
    shim,
    `#!/usr/bin/env bash\nexec "${process.execPath}" "${cliEntry}" "$@"\n`,
  );
  await chmod(shim, 0o755);
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

function waitForServer(
  proc: ChildProcess,
  port: number,
  timeout = 5_000,
): Promise<boolean> {
  return new Promise((resolve) => {
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

describe("dirsql CLI --config <native config file>", () => {
  it.each(["js", "mjs", "cjs"] as const)(
    "starts an HTTP server serving the config tables (.%s)",
    async (ext) => {
      const configPath = join(CONFIG_DIR, `dirsql.config.${ext}`);
      const port = await freePort();
      const proc = spawn(
        BINARY,
        ["--config", configPath, "--host", "127.0.0.1", "--port", String(port)],
        {
          stdio: "pipe",
          env: {
            ...process.env,
            PATH: `${SHIM_DIR}:${process.env.PATH ?? ""}`,
          },
        },
      );

      // Drain stdout/stderr and capture both. With `stdio: "pipe"` and
      // no consumer the kernel pipe buffer (~64 KiB on Linux) fills and
      // the binary blocks on write. Capture both so any diagnostic
      // (binary's stdout banner, interpret subprocess errors via the
      // binary's inherited stderr) shows up in the failure message.
      let stdout = "";
      let stderr = "";
      let exited: {
        code: number | null;
        signal: NodeJS.Signals | null;
      } | null = null;
      proc.stdout?.on("data", (chunk: Buffer) => {
        stdout += chunk.toString();
      });
      proc.stderr?.on("data", (chunk: Buffer) => {
        stderr += chunk.toString();
      });
      proc.on("exit", (code, signal) => {
        exited = { code, signal };
      });

      try {
        const ok = await waitForServer(proc, port);
        if (!ok) {
          const exitInfo = exited
            ? `proc exited (code=${exited.code}, signal=${exited.signal})`
            : "proc still alive (timeout)";
          throw new Error(
            `dirsql server did not start with --config .${ext}; ${exitInfo}\n` +
              `--- stdout ---\n${stdout}\n` +
              `--- stderr ---\n${stderr}`,
          );
        }
        const rows = await httpQuery(
          port,
          "SELECT title FROM papers ORDER BY title",
        );
        expect(rows).toEqual([{ title: "Alpha" }, { title: "Beta" }]);
      } finally {
        proc.kill("SIGTERM");
        await new Promise<void>((resolve) =>
          proc.once("exit", () => resolve()),
        );
      }
    },
    10_000,
  );
});
