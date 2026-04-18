// Integration tests for the `dirsql` CLI launcher. Spawns the compiled
// launcher against a fake `@dirsql/cli-*` install tree so
// `require.resolve` + `spawnSync` run for real.

import { mkdirSync, mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { fakeInstallRoot } from "./fakeInstallRoot.js";
import { runLauncher } from "./runLauncher.js";

describe("dirsql CLI launcher", () => {
  describe("when a matching platform package is installed", () => {
    it("forwards argv to the resolved binary and exits 0", () => {
      const root = fakeInstallRoot();
      const result = runLauncher(root, ["--help", "hello"]);
      expect(result.status, result.stderr).toBe(0);
      expect(result.stdout).toMatch(/args=--help hello/);
    });

    it("propagates a non-zero child exit code", () => {
      const root = fakeInstallRoot({ exitCode: 42 });
      const result = runLauncher(root, []);
      expect(result.status, result.stderr).toBe(42);
    });
  });

  describe("when the platform package is missing", () => {
    it("exits 1 with a helpful error pointing at --no-optional", () => {
      const empty = mkdtempSync(join(tmpdir(), "dirsql-empty-"));
      mkdirSync(join(empty, "node_modules"), { recursive: true });
      const result = runLauncher(empty, []);
      expect(result.status).toBe(1);
      expect(result.stderr).toMatch(/is not installed/);
      expect(result.stderr).toMatch(/--no-optional|--ignore-optional/);
    });
  });
});
