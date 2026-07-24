// Config-driven construction: `new DirSQL(configPath)`.
//
// Config-defined tables produce one row per matched file. Each row's columns
// come from filesystem facts: the stat virtuals (`path`, `basename`, `dir`,
// `ext`, `size`, `mtime`, `ctime`). Content interpretation is intentionally
// out of scope; for that, register a programmatic Table with your own onFile
// function.

import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { DirSQL } from "dirsql";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

// Renamed from `writeFile` to avoid shadowing the `node:fs/promises` import;
// it ensures the parent directory exists before writing.
async function seedFile(path: string, content: string): Promise<void> {
  await mkdir(dirname(path), { recursive: true });
  await writeFile(path, content);
}

// Each fixture emits its own filesystem-fact columns via an `on-file` hook
// rather than relying on the core injecting them. Hook output overrides
// injection, so results are identical while injection remains and stay green
// once it is removed. `{path}` is the absolute path, `{root}` the index root;
// the relative path is `{path}` with the `{root}/` prefix stripped.
const pathHook = `on-file = '''sh -c 'rel=\${1#"$2"/}; printf "[{\\"path\\":\\"%s\\"}]" "$rel"' sh {path} {root}'''`;
const pathBasenameHook = `on-file = '''sh -c 'rel=\${1#"$2"/}; base=\${1##*/}; printf "[{\\"path\\":\\"%s\\",\\"basename\\":\\"%s\\"}]" "$rel" "$base"' sh {path} {root}'''`;
const basenameHook = `on-file = '''sh -c 'printf "[{\\"basename\\":\\"%s\\"}]" "\${1##*/}"' sh {path}'''`;
const statHook = `on-file = '''sh -c 'rel=\${1#"$2"/}; base=\${1##*/}; case "$rel" in */*) dir=\${rel%/*};; *) dir="";; esac; ext=\${base##*.}; [ "$ext" = "$base" ] && ext=""; size=$(wc -c < "$1" | tr -d " "); mtime=$(stat -c %Y "$1"); printf "[{\\"path\\":\\"%s\\",\\"basename\\":\\"%s\\",\\"dir\\":\\"%s\\",\\"ext\\":\\"%s\\",\\"size\\":%s,\\"mtime\\":%s}]" "$rel" "$base" "$dir" "$ext" "$size" "$mtime"' sh {path} {root}'''`;

describe("new DirSQL(configPath)", () => {
  let dir: string;
  let configPath: string;

  beforeEach(async () => {
    dir = await mkdtemp(join(tmpdir(), "dirsql-fromconfig-"));
    configPath = join(dir, ".dirsql.toml");
  });

  afterEach(async () => {
    await rm(dir, { recursive: true, force: true });
  });

  it("produces one row per matched file with stat virtuals", async () => {
    await seedFile(join(dir, "items", "a.csv"), "anything");
    await seedFile(join(dir, "items", "b.csv"), "anything");
    await seedFile(
      configPath,
      `
[[table]]
ddl = "CREATE TABLE items (path TEXT, basename TEXT)"
glob = "items/*.csv"
${pathBasenameHook}
`,
    );

    const db = new DirSQL({ root: dir, config: configPath });
    await db.ready;
    const rows = await db.query(
      "SELECT path, basename FROM items ORDER BY path",
    );
    expect(rows).toHaveLength(2);
    expect(rows[0].path).toBe("items/a.csv");
    expect(rows[0].basename).toBe("a.csv");
    expect(rows[1].path).toBe("items/b.csv");
    expect(rows[1].basename).toBe("b.csv");
  });

  it("rejects a glob placeholder that collides with a declared column", async () => {
    await seedFile(join(dir, "comments", "thread-1", "a.txt"), "x");
    await seedFile(
      configPath,
      `
[[table]]
ddl = "CREATE TABLE comments (thread_id TEXT, basename TEXT)"
glob = "comments/{thread_id}/*.txt"
`,
    );

    const db = new DirSQL({ root: dir, config: configPath });
    await expect(db.ready).rejects.toThrow(/thread_id/);
  });

  it("treats a non-colliding placeholder as a wildcard", async () => {
    await seedFile(join(dir, "comments", "thread-1", "a.txt"), "x");
    await seedFile(join(dir, "comments", "thread-2", "b.txt"), "x");
    await seedFile(
      configPath,
      `
[[table]]
ddl = "CREATE TABLE comments (path TEXT, basename TEXT)"
glob = "comments/{thread_id}/*.txt"
${pathBasenameHook}
`,
    );

    const db = new DirSQL({ root: dir, config: configPath });
    await db.ready;
    const rows = await db.query(
      "SELECT basename FROM comments ORDER BY basename",
    );
    expect(rows).toHaveLength(2);
    expect(rows[0].basename).toBe("a.txt");
    expect(rows[1].basename).toBe("b.txt");
    expect(rows[0].thread_id).toBeUndefined();
  });

  it("exposes the full set of stat virtuals when declared in DDL", async () => {
    const body = "# title\nhello world\n";
    await seedFile(join(dir, "docs", "readme.md"), body);
    await seedFile(
      configPath,
      `
[[table]]
ddl = "CREATE TABLE files (path TEXT, basename TEXT, dir TEXT, ext TEXT, size INTEGER, mtime INTEGER)"
glob = "docs/*.md"
${statHook}
`,
    );

    const db = new DirSQL({ root: dir, config: configPath });
    await db.ready;
    const rows = await db.query(
      "SELECT path, basename, dir, ext, size, mtime FROM files",
    );
    expect(rows).toHaveLength(1);
    const r = rows[0];
    expect(r.path).toBe("docs/readme.md");
    expect(r.basename).toBe("readme.md");
    expect(r.dir).toBe("docs");
    expect(r.ext).toBe("md");
    expect(r.size).toBe(body.length);
    expect(typeof r.mtime).toBe("number");
    expect(r.mtime as number).toBeGreaterThan(0);
  });

  it("respects ignore patterns from config", async () => {
    await seedFile(join(dir, "data", "good.json"), "{}");
    await seedFile(join(dir, "data", "node_modules", "bad.json"), "{}");
    await seedFile(
      configPath,
      `
[dirsql]
ignore = ["**/node_modules/**"]

[[table]]
ddl = "CREATE TABLE items (path TEXT)"
glob = "data/**/*.json"
${pathHook}
`,
    );

    const db = new DirSQL({ root: dir, config: configPath });
    await db.ready;
    const rows = await db.query("SELECT path FROM items");
    expect(rows).toHaveLength(1);
    expect(rows[0].path).toBe("data/good.json");
  });

  it("loads multiple tables", async () => {
    await seedFile(join(dir, "posts", "hello.txt"), "x");
    await seedFile(join(dir, "authors", "alice.txt"), "x");
    await seedFile(
      configPath,
      `
[[table]]
ddl = "CREATE TABLE posts (basename TEXT)"
glob = "posts/*.txt"
${basenameHook}

[[table]]
ddl = "CREATE TABLE authors (basename TEXT)"
glob = "authors/*.txt"
${basenameHook}
`,
    );

    const db = new DirSQL({ root: dir, config: configPath });
    await db.ready;
    const posts = await db.query("SELECT basename FROM posts");
    const authors = await db.query("SELECT basename FROM authors");
    expect(posts).toHaveLength(1);
    expect(authors).toHaveLength(1);
    expect(posts[0].basename).toBe("hello.txt");
    expect(authors[0].basename).toBe("alice.txt");
  });

  it("rejects missing config files", async () => {
    const missing = join(dir, "nonexistent.toml");
    const db = new DirSQL(missing);
    await expect(db.ready).rejects.toThrow();
  });

  it("rejects invalid TOML", async () => {
    await seedFile(configPath, "this is not valid [[[");
    const db = new DirSQL(configPath);
    await expect(db.ready).rejects.toThrow();
  });

  it("rejects table entries missing ddl", async () => {
    await seedFile(
      configPath,
      `
[[table]]
glob = "*.json"
`,
    );
    const db = new DirSQL(configPath);
    await expect(db.ready).rejects.toThrow();
  });
});
