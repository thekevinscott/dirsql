// Gap-filling tests for documented features previously untested on the TS
// SDK (#294 test parity).
//
// Mirrors packages/python/tests/integration/dirsql_test.py
// (describe_relaxed_schema, describe_extract_receives_path) and
// packages/rust/tests/sdk.rs (it_ignores_extra_keys_by_default,
// it_fills_missing_keys_with_null, it_raises_on_missing_keys_in_strict_mode).
// The strict extra-keys / exact-match cases already live in index.test.ts.
//
// NOTE on `bytes -> BLOB` (guide/tables.md "Supported value types"): the
// mapping is documented for Python and covered there and in Rust
// (docs_gaps_test.py / docs_gaps.rs). The TS binding has no
// Buffer -> BLOB mapping (a Buffer coerces to its string representation),
// so there is deliberately no TS mirror — the drift is tracked in
// PARITY.md's test coverage matrix.

import { readFileSync } from "node:fs";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { isAbsolute, join } from "node:path";
import { DirSQL } from "dirsql";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

let dir: string;

beforeEach(async () => {
  dir = await mkdtemp(join(tmpdir(), "dirsql-gaps-"));
});

afterEach(async () => {
  await rm(dir, { recursive: true, force: true });
});

// Docs (guide/tables.md / guide/config.md "Strict Mode"): the default
// (relaxed) mode drops extract keys that aren't declared in the DDL and
// fills declared-but-missing columns with NULL.
describe("DirSQL relaxed schema (default)", () => {
  it("ignores extra keys by default", async () => {
    await writeFile(
      join(dir, "a.json"),
      JSON.stringify({ name: "apple", color: "red" }),
    );

    const db = new DirSQL({
      root: dir,
      tables: [
        {
          ddl: "CREATE TABLE items (name TEXT)",
          glob: "*.json",
          extract: (filePath: string) => [
            JSON.parse(readFileSync(filePath, "utf8")),
          ],
        },
      ],
    });

    const rows = await db.query("SELECT * FROM items");
    expect(rows).toEqual([{ name: "apple" }]);
  });

  it("fills missing keys with NULL", async () => {
    await writeFile(join(dir, "a.json"), JSON.stringify({ name: "apple" }));

    const db = new DirSQL({
      root: dir,
      tables: [
        {
          ddl: "CREATE TABLE items (name TEXT, color TEXT)",
          glob: "*.json",
          extract: (filePath: string) => [
            JSON.parse(readFileSync(filePath, "utf8")),
          ],
        },
      ],
    });

    const rows = await db.query("SELECT * FROM items");
    expect(rows).toEqual([{ name: "apple", color: null }]);
  });
});

// Docs: strict mode errors on declared columns the extract row is missing.
// Completes the strict-mode triple — the extra-keys and exact-match cases
// live in index.test.ts.
describe("DirSQL strict mode (missing keys)", () => {
  it("rejects rows with missing keys when strict is true", async () => {
    await writeFile(join(dir, "a.json"), JSON.stringify({ name: "apple" }));

    const db = new DirSQL({
      root: dir,
      tables: [
        {
          ddl: "CREATE TABLE items (name TEXT, color TEXT)",
          glob: "*.json",
          extract: (filePath: string) => [
            JSON.parse(readFileSync(filePath, "utf8")),
          ],
          strict: true,
        },
      ],
    });

    await expect(db.ready).rejects.toThrow();
  });
});

// Docs (guide/tables.md "extract"): the callback receives the filesystem
// path of the matched file (absolute when the root is absolute); dirsql
// does not read file contents itself.
describe("DirSQL extract path argument", () => {
  it("passes the absolute path of the matched file", async () => {
    await writeFile(join(dir, "item.json"), JSON.stringify({ name: "x" }));

    const seenPaths: string[] = [];
    const db = new DirSQL({
      root: dir, // mkdtemp returns an absolute path
      tables: [
        {
          ddl: "CREATE TABLE items (name TEXT)",
          glob: "*.json",
          extract: (filePath: string) => {
            seenPaths.push(filePath);
            return [JSON.parse(readFileSync(filePath, "utf8"))];
          },
        },
      ],
    });
    await db.ready;

    expect(seenPaths).toHaveLength(1);
    expect(isAbsolute(seenPaths[0])).toBe(true);
    expect(seenPaths[0].endsWith("item.json")).toBe(true);
  });
});
