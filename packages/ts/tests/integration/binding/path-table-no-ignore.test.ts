import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DirSQL } from "dirsql";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

// Path-table scans respect .gitignore by default (#742); `noIgnore: true`
// opts back in to the ignored files while the built-in floor (node_modules,
// .git) still applies. The behavior lives in the Rust core -- these tests
// prove the constructor option crosses the napi boundary.
describe("path-table gitignore opt-out (#746)", () => {
  let dir: string;

  beforeEach(async () => {
    dir = await mkdtemp(join(tmpdir(), "dirsql-746-"));
    await mkdir(join(dir, "dist"), { recursive: true });
    await mkdir(join(dir, "src"), { recursive: true });
    await mkdir(join(dir, "node_modules", "pkg"), { recursive: true });
    await writeFile(join(dir, ".gitignore"), "dist/\n*.log\n");
    await writeFile(join(dir, "dist", "bundle.js"), "js");
    await writeFile(join(dir, "src", "app.js"), "js");
    await writeFile(join(dir, "debug.log"), "log");
    await writeFile(join(dir, "node_modules", "pkg", "index.js"), "js");
  });

  afterEach(async () => {
    await rm(dir, { recursive: true, force: true });
  });

  const paths = async (db: DirSQL) => {
    const rows = await db.query("SELECT path FROM './'");
    return rows.map((r) => r.path).sort();
  };

  it("excludes gitignored files by default", async () => {
    const scanned = await paths(new DirSQL({ root: dir }));

    expect(scanned).not.toContain("dist/bundle.js");
    expect(scanned).not.toContain("debug.log");
    expect(scanned).toContain("src/app.js");
  });

  it("restores gitignored files under noIgnore but keeps the built-in floor", async () => {
    const scanned = await paths(new DirSQL({ root: dir, noIgnore: true }));

    expect(scanned).toContain("dist/bundle.js");
    expect(scanned).toContain("debug.log");
    expect(scanned).not.toContain("node_modules/pkg/index.js");
  });
});
