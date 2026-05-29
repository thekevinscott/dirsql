// Integration tests for structured column definitions (issue #202).
//
// These exercise the new `{ name, glob, columns: [...] }` TableDef shape that
// replaces the raw `ddl` string. Each test builds a real DirSQL over a temp
// directory and inspects the resulting SQLite schema through the read-only
// `query()` API (`pragma_table_info`, `pragma_index_list`, `sqlite_master`),
// so a passing test proves the Rust core generated the expected `CREATE TABLE`
// from the structured shape.
//
// Written test-first (red/green): until the napi binding accepts `columns`
// and the core grows `Table::to_ddl`, every test here is expected to fail.

import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DirSQL } from "dirsql";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

type Row = Record<string, unknown>;

async function columnsOf(
  db: DirSQL,
  table: string,
): Promise<Record<string, Row>> {
  const rows = await db.query(
    `SELECT name, type, "notnull" AS nn, dflt_value, pk FROM pragma_table_info('${table}')`,
  );
  const out: Record<string, Row> = {};
  for (const r of rows) {
    const name = String(r.name);
    if (!name.startsWith("_dirsql_")) out[name] = r;
  }
  return out;
}

async function tableSql(db: DirSQL, table: string): Promise<string> {
  const rows = await db.query(
    `SELECT sql FROM sqlite_master WHERE type='table' AND name='${table}'`,
  );
  return String(rows[0]?.sql ?? "");
}

async function indexesOf(
  db: DirSQL,
  table: string,
): Promise<Record<string, boolean>> {
  const rows = await db.query(
    `SELECT name, "unique" AS uq FROM pragma_index_list('${table}')`,
  );
  const out: Record<string, boolean> = {};
  for (const r of rows) {
    out[String(r.name)] = Boolean(r.uq);
  }
  return out;
}

describe("structured columns", () => {
  let dir: string;

  beforeEach(() => {
    dir = mkdtempSync(join(tmpdir(), "dirsql-cols-"));
  });

  afterEach(() => {
    rmSync(dir, { recursive: true, force: true });
  });

  describe("deprecation", () => {
    // Place first so this file's process sees `ddl` for the first time here.
    it("warns when the legacy ddl shape is used", async () => {
      const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
      try {
        const db = new DirSQL({
          root: dir,
          tables: [
            {
              ddl: "CREATE TABLE t (x TEXT)",
              glob: "*.md",
              extract: () => [],
            },
          ],
        });
        await db.ready;
        expect(warn).toHaveBeenCalled();
      } finally {
        warn.mockRestore();
      }
    });

    it("rejects when both ddl and columns are supplied", async () => {
      const db = new DirSQL({
        root: dir,
        tables: [
          {
            name: "t",
            ddl: "CREATE TABLE t (x TEXT)",
            glob: "*.md",
            columns: [{ name: "x", type: "TEXT" }],
            extract: () => [],
          },
        ],
      });
      await expect(db.ready).rejects.toThrow();
    });
  });

  describe("basic", () => {
    it("creates a table from name + columns and round-trips data", async () => {
      writeFileSync(join(dir, "a.md"), "# hi");
      const db = new DirSQL({
        root: dir,
        tables: [
          {
            name: "docs",
            glob: "*.md",
            columns: [
              { name: "title", type: "TEXT" },
              { name: "body", type: "TEXT" },
            ],
            extract: () => [{ title: "hi", body: "world" }],
          },
        ],
      });
      const rows = await db.query("SELECT title, body FROM docs");
      expect(rows).toEqual([{ title: "hi", body: "world" }]);
    });

    it("supports every SQLite storage type", async () => {
      const db = new DirSQL({
        root: dir,
        tables: [
          {
            name: "t",
            glob: "*.bin",
            columns: [
              { name: "a", type: "TEXT" },
              { name: "b", type: "INTEGER" },
              { name: "c", type: "REAL" },
              { name: "d", type: "BLOB" },
              { name: "e", type: "NUMERIC" },
            ],
            extract: () => [],
          },
        ],
      });
      const cols = await columnsOf(db, "t");
      expect(cols.a?.type).toBe("TEXT");
      expect(cols.b?.type).toBe("INTEGER");
      expect(cols.c?.type).toBe("REAL");
      expect(cols.d?.type).toBe("BLOB");
      expect(cols.e?.type).toBe("NUMERIC");
    });
  });

  describe("column constraints", () => {
    it("marks NOT NULL columns", async () => {
      const db = new DirSQL({
        root: dir,
        tables: [
          {
            name: "t",
            glob: "*.md",
            columns: [{ name: "title", type: "TEXT", notNull: true }],
            extract: () => [],
          },
        ],
      });
      const cols = await columnsOf(db, "t");
      expect(cols.title?.nn).toBe(1);
    });

    it("marks PRIMARY KEY columns", async () => {
      const db = new DirSQL({
        root: dir,
        tables: [
          {
            name: "t",
            glob: "*.md",
            columns: [{ name: "id", type: "TEXT", primaryKey: true }],
            extract: () => [],
          },
        ],
      });
      const cols = await columnsOf(db, "t");
      expect(cols.id?.pk).toBe(1);
    });

    it("emits a scalar DEFAULT", async () => {
      const db = new DirSQL({
        root: dir,
        tables: [
          {
            name: "t",
            glob: "*.md",
            columns: [{ name: "title", type: "TEXT", default: "untitled" }],
            extract: () => [],
          },
        ],
      });
      const cols = await columnsOf(db, "t");
      expect(cols.title?.dflt_value).toBe("'untitled'");
    });
  });

  describe("sql escape hatch", () => {
    it("supports an expression DEFAULT", async () => {
      const db = new DirSQL({
        root: dir,
        tables: [
          {
            name: "t",
            glob: "*.md",
            columns: [
              {
                name: "ingested_at",
                type: "INTEGER",
                default: { sql: "strftime('%s', 'now')" },
              },
            ],
            extract: () => [],
          },
        ],
      });
      const cols = await columnsOf(db, "t");
      expect(cols.ingested_at?.dflt_value).toBe("strftime('%s', 'now')");
    });

    it("supports a CHECK constraint", async () => {
      const db = new DirSQL({
        root: dir,
        tables: [
          {
            name: "t",
            glob: "*.md",
            columns: [
              {
                name: "body",
                type: "TEXT",
                check: { sql: "length(body) > 0" },
              },
            ],
            extract: () => [],
          },
        ],
      });
      const sql = await tableSql(db, "t");
      expect(sql).toContain("CHECK");
      expect(sql).toContain("length(body) > 0");
    });

    it("supports a GENERATED column", async () => {
      const db = new DirSQL({
        root: dir,
        tables: [
          {
            name: "t",
            glob: "*.md",
            columns: [
              { name: "body", type: "TEXT" },
              {
                name: "body_len",
                type: "INTEGER",
                generated: { sql: "length(body)", mode: "stored" },
              },
            ],
            extract: () => [],
          },
        ],
      });
      const sql = (await tableSql(db, "t")).toUpperCase();
      expect(sql).toContain("LENGTH(BODY)");
      expect(sql).toContain("STORED");
    });
  });

  describe("table-level", () => {
    it("supports a composite PRIMARY KEY", async () => {
      const db = new DirSQL({
        root: dir,
        tables: [
          {
            name: "t",
            glob: "*.md",
            columns: [
              { name: "a", type: "TEXT" },
              { name: "b", type: "TEXT" },
            ],
            primaryKey: ["a", "b"],
            extract: () => [],
          },
        ],
      });
      const cols = await columnsOf(db, "t");
      expect(cols.a?.pk).toBe(1);
      expect(cols.b?.pk).toBe(2);
    });

    it("supports declared indexes", async () => {
      const db = new DirSQL({
        root: dir,
        tables: [
          {
            name: "t",
            glob: "*.md",
            columns: [{ name: "title", type: "TEXT" }],
            indexes: [{ name: "idx_title", columns: ["title"], unique: true }],
            extract: () => [],
          },
        ],
      });
      const idx = await indexesOf(db, "t");
      expect(idx.idx_title).toBe(true);
    });

    it("supports WITHOUT ROWID", async () => {
      const db = new DirSQL({
        root: dir,
        tables: [
          {
            name: "t",
            glob: "*.md",
            columns: [{ name: "id", type: "TEXT", primaryKey: true }],
            withoutRowid: true,
            extract: () => [],
          },
        ],
      });
      expect((await tableSql(db, "t")).toUpperCase()).toContain(
        "WITHOUT ROWID",
      );
    });

    it("supports STRICT table mode", async () => {
      const db = new DirSQL({
        root: dir,
        tables: [
          {
            name: "t",
            glob: "*.md",
            columns: [{ name: "title", type: "TEXT" }],
            strictTypes: true,
            extract: () => [],
          },
        ],
      });
      expect((await tableSql(db, "t")).toUpperCase()).toContain("STRICT");
    });
  });
});
