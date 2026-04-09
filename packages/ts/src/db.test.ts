import { describe, it, expect } from "vitest";
import { Db, parseTableName } from "./db.js";

describe("Db", () => {
  describe("createTable", () => {
    it("creates a table from DDL", () => {
      const db = new Db();
      db.createTable(
        "CREATE TABLE comments (id TEXT PRIMARY KEY, body TEXT, resolved INTEGER)",
      );
      const rows = db.query("SELECT * FROM comments");
      expect(rows).toEqual([]);
    });

    it("throws on invalid DDL", () => {
      const db = new Db();
      expect(() => db.createTable("NOT VALID SQL")).toThrow();
    });

    it("injects tracking columns", () => {
      const db = new Db();
      db.createTable("CREATE TABLE t (id TEXT)");
      db.insertRow("t", { id: "1" }, "test.json", 0);

      const rows = db.query("SELECT * FROM t");
      expect(rows).toHaveLength(1);
      expect(rows[0]).toHaveProperty("id");
      expect(rows[0]).not.toHaveProperty("_dirsql_file_path");
      expect(rows[0]).not.toHaveProperty("_dirsql_row_index");
    });
  });

  describe("insertRow / query", () => {
    it("inserts and queries rows", () => {
      const db = new Db();
      db.createTable("CREATE TABLE docs (title TEXT, draft INTEGER)");
      db.insertRow("docs", { title: "Hello", draft: 0 }, "docs/hello.md", 0);

      const results = db.query("SELECT title, draft FROM docs");
      expect(results).toHaveLength(1);
      expect(results[0].title).toBe("Hello");
      expect(results[0].draft).toBe(0);
    });

    it("inserts multiple rows from same file", () => {
      const db = new Db();
      db.createTable("CREATE TABLE events (action TEXT, ts INTEGER)");

      const actions = ["created", "resolved", "reopened"];
      for (let i = 0; i < actions.length; i++) {
        db.insertRow("events", { action: actions[i], ts: i }, "thread.jsonl", i);
      }

      const results = db.query("SELECT action FROM events ORDER BY ts");
      expect(results).toHaveLength(3);
      expect(results[0].action).toBe("created");
      expect(results[2].action).toBe("reopened");
    });
  });

  describe("deleteRowsByFile", () => {
    it("deletes rows by file path", () => {
      const db = new Db();
      db.createTable("CREATE TABLE comments (id TEXT, body TEXT)");

      db.insertRow("comments", { id: "1", body: "text" }, "a.jsonl", 0);
      db.insertRow("comments", { id: "2", body: "text" }, "a.jsonl", 1);
      db.insertRow("comments", { id: "3", body: "text" }, "b.jsonl", 0);

      const deleted = db.deleteRowsByFile("comments", "a.jsonl");
      expect(deleted).toBe(2);

      const results = db.query("SELECT id FROM comments");
      expect(results).toHaveLength(1);
      expect(results[0].id).toBe("3");
    });
  });

  describe("query with WHERE", () => {
    it("filters with WHERE clause", () => {
      const db = new Db();
      db.createTable("CREATE TABLE items (name TEXT, count INTEGER)");

      const data = [
        ["apple", 5],
        ["banana", 0],
        ["cherry", 3],
      ] as const;
      for (let i = 0; i < data.length; i++) {
        db.insertRow(
          "items",
          { name: data[i][0], count: data[i][1] },
          "items.json",
          i,
        );
      }

      const results = db.query(
        "SELECT name FROM items WHERE count > 0 ORDER BY name",
      );
      expect(results).toHaveLength(2);
      expect(results[0].name).toBe("apple");
      expect(results[1].name).toBe("cherry");
    });
  });
});

describe("parseTableName", () => {
  it("parses simple CREATE TABLE", () => {
    expect(parseTableName("CREATE TABLE comments (id TEXT)")).toBe("comments");
  });

  it("parses CREATE TABLE IF NOT EXISTS", () => {
    expect(
      parseTableName("CREATE TABLE IF NOT EXISTS comments (id TEXT)"),
    ).toBe("comments");
  });

  it("handles no space before paren", () => {
    expect(parseTableName("CREATE TABLE t(id TEXT)")).toBe("t");
  });

  it("returns null for invalid DDL", () => {
    expect(parseTableName("NOT A DDL")).toBeNull();
  });

  it("returns null for empty after CREATE TABLE", () => {
    expect(parseTableName("CREATE TABLE ")).toBeNull();
  });
});
