// Binding-tier tests (real napi core, real fs) for repeatable `config` (#589).
//
// `new DirSQL({ config })` accepts a single path or an array of paths that
// merge in order (matching the Rust builder's repeatable `.config()` and the
// CLI's repeatable `-c`); a single string / `new DirSQL(path)` is unchanged.

import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DirSQL } from "dirsql";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

// The `on-file` hook emits `basename` itself rather than relying on the core
// injecting it, so the fixture stays green once stat-fact injection is removed;
// hook output overrides injection, keeping results identical meanwhile.
const basenameHook = `on-file = '''sh -c 'printf "[{\\"basename\\":\\"%s\\"}]" "\${1##*/}"' sh {path}'''`;

function tableConfig(name: string, glob: string): string {
  return `[[table]]\nname = "${name}"\nddl = "CREATE TABLE ${name} (basename TEXT)"\nglob = "${glob}"\n${basenameHook}\n`;
}

describe("repeatable config", () => {
  let dir: string;

  beforeEach(async () => {
    dir = await mkdtemp(join(tmpdir(), "dirsql-repeatcfg-"));
  });

  afterEach(async () => {
    await rm(dir, { recursive: true, force: true });
  });

  it("merges tables from multiple config files", async () => {
    await writeFile(join(dir, "a.json"), "{}");
    const cfgA = join(dir, "a.dirsql.toml");
    const cfgB = join(dir, "b.dirsql.toml");
    await writeFile(cfgA, tableConfig("alpha", "*.json"));
    await writeFile(cfgB, tableConfig("beta", "*.json"));

    const db = new DirSQL({ root: dir, config: [cfgA, cfgB] });
    for (const table of ["alpha", "beta"]) {
      const rows = await db.query(`SELECT basename FROM ${table}`);
      expect(rows.map((r) => r.basename)).toEqual(["a.json"]);
    }
  });

  it("accepts a single config string unchanged", async () => {
    await writeFile(join(dir, "a.json"), "{}");
    const cfg = join(dir, ".dirsql.toml");
    await writeFile(cfg, tableConfig("alpha", "*.json"));

    const db = new DirSQL({ root: dir, config: cfg });
    const rows = await db.query("SELECT basename FROM alpha");
    expect(rows.map((r) => r.basename)).toEqual(["a.json"]);
  });
});
