// Binding-tier tests (real core, real fs) for the declared table `name`.
//
// A table's name is declared, never derived from `ddl`. A `[[table]]` entry
// without `name` fails to load, and a `name` the entry's `ddl` never creates
// fails at load time -- checked against SQLite's own catalog, before any
// ingestion.

import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { DirSQL } from "dirsql";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

// Emits the file's root-relative `path`. `{path}` is the absolute path,
// `{root}` the index root.
const pathHook = `on-file = '''sh -c 'rel=\${1#"$2"/}; printf "[{\\"path\\":\\"%s\\"}]" "$rel"' sh {path} {root}'''`;

async function seedFile(path: string, content: string): Promise<void> {
  await mkdir(dirname(path), { recursive: true });
  await writeFile(path, content);
}

describe("declared [[table]] name", () => {
  let dir: string;
  let configPath: string;

  beforeEach(async () => {
    dir = await mkdtemp(join(tmpdir(), "dirsql-declared-name-"));
    configPath = join(dir, ".dirsql.toml");
    await seedFile(join(dir, "data", "a.csv"), "anything");
  });

  afterEach(async () => {
    await rm(dir, { recursive: true, force: true });
  });

  it("registers a config table under its declared name", async () => {
    await seedFile(
      configPath,
      `
[[table]]
name = "notes"
ddl = "CREATE TABLE notes (path TEXT)"
glob = "data/*.csv"
${pathHook}
`,
    );

    const db = new DirSQL({ root: dir, config: configPath });
    const rows = await db.query("SELECT path FROM notes");
    expect(rows.map((r) => r.path)).toEqual(["data/a.csv"]);
  });

  it("rejects a [[table]] entry with no name", async () => {
    await seedFile(
      configPath,
      `
[[table]]
ddl = "CREATE TABLE notes (path TEXT)"
glob = "data/*.csv"
${pathHook}
`,
    );

    const db = new DirSQL({ root: dir, config: configPath });
    await expect(db.ready).rejects.toThrow(/name/);
  });

  it("rejects a declared name the ddl never creates", async () => {
    await seedFile(
      configPath,
      `
[[table]]
name = "messages"
ddl = "CREATE TABLE notes (path TEXT)"
glob = "data/*.csv"
${pathHook}
`,
    );

    const db = new DirSQL({ root: dir, config: configPath });
    await expect(db.ready).rejects.toThrow(/table 'messages'/);
  });
});
