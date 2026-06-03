import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { loadApp } from "./loadApp.js";

describe("loadApp", () => {
  let dir: string;

  beforeEach(() => {
    dir = mkdtempSync(join(tmpdir(), "dirsql-loadapp-"));
  });

  afterEach(() => {
    rmSync(dir, { recursive: true, force: true });
  });

  it("returns the default export of an .mjs config", async () => {
    const path = join(dir, "config.mjs");
    writeFileSync(path, "export default { sentinel: 'value' };\n");
    const app = (await loadApp(path)) as unknown as { sentinel: string };
    expect(app.sentinel).toBe("value");
  });

  it("returns the module.exports of a .cjs config", async () => {
    const path = join(dir, "config.cjs");
    writeFileSync(path, "module.exports = { sentinel: 'cjs' };\n");
    const app = (await loadApp(path)) as unknown as { sentinel: string };
    expect(app.sentinel).toBe("cjs");
  });

  it("throws a path-aware error when the module has no default export", async () => {
    const path = join(dir, "no_default.mjs");
    writeFileSync(path, "export const x = 1;\n");
    await expect(loadApp(path)).rejects.toThrow(
      /must default-export a DirSQL instance/,
    );
    await expect(loadApp(path)).rejects.toThrow(path);
  });

  it("propagates errors thrown during module evaluation", async () => {
    const path = join(dir, "boom.mjs");
    writeFileSync(path, "throw new Error('synthetic boom');\n");
    await expect(loadApp(path)).rejects.toThrow(/synthetic boom/);
  });

  it("rejects when the file does not exist", async () => {
    await expect(loadApp(join(dir, "nope.mjs"))).rejects.toThrow();
  });
});
