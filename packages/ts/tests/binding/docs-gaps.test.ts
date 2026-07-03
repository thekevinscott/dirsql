// Gap-filling tests for documented features previously untested on the TS
// SDK (#294 test parity).
//
// Mirrors packages/python/tests/binding/dirsql_test.py
// (describe_relaxed_schema, describe_extract_receives_path) and
// packages/rust/tests/sdk.rs (it_ignores_extra_keys_by_default,
// it_fills_missing_keys_with_null, it_raises_on_missing_keys_in_strict_mode).
// The strict extra-keys / exact-match cases already live in index.test.ts.
//
// The `Buffer -> BLOB` describe below mirrors
// docs_gaps_test.py::it_maps_python_bytes_to_sqlite_blob and
// packages/rust/tests/docs_gaps.rs::extract_blob_values_round_trip_via_sdk
// (#343 parity restoration).

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

// Docs (guide/tables.md "Supported value types"): `Buffer` / `Uint8Array`
// -> SQLite BLOB; BLOB columns come back from `query()` as `Buffer`.
// Round-trips the same payload as the Python/Rust mirrors.
describe("DirSQL Buffer -> BLOB", () => {
  const payload = Buffer.from([0x00, 0x01, 0x02, 0xff, 0xfe]);

  async function roundTrip(data: unknown): Promise<unknown> {
    await writeFile(join(dir, "marker.json"), "{}");
    const db = new DirSQL({
      root: dir,
      tables: [
        {
          ddl: "CREATE TABLE blobs (name TEXT, data BLOB)",
          glob: "*.json",
          extract: () => [{ name: "bin", data }],
        },
      ],
    });
    const rows = await db.query("SELECT * FROM blobs");
    expect(rows).toHaveLength(1);
    expect(rows[0].name).toBe("bin");
    return rows[0].data;
  }

  it("round-trips a Buffer through a BLOB column", async () => {
    const data = await roundTrip(payload);
    expect(Buffer.isBuffer(data)).toBe(true);
    expect(Buffer.compare(data as Buffer, payload)).toBe(0);
  });

  it("maps a Uint8Array to a BLOB and returns it as a Buffer", async () => {
    const data = await roundTrip(new Uint8Array(payload));
    expect(Buffer.isBuffer(data)).toBe(true);
    expect(Buffer.compare(data as Buffer, payload)).toBe(0);
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
