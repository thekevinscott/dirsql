import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { DirSQL } from "dirsql";

describe("DirSQL", () => {
  let dir: string;

  beforeEach(() => {
    dir = mkdtempSync(join(tmpdir(), "dirsql-test-"));
    mkdirSync(join(dir, "data"), { recursive: true });
    writeFileSync(
      join(dir, "data", "users.json"),
      JSON.stringify([
        { name: "Alice", age: 30 },
        { name: "Bob", age: 25 },
      ]),
    );
    writeFileSync(
      join(dir, "data", "products.json"),
      JSON.stringify([
        { name: "Widget", price: 9.99 },
        { name: "Gadget", price: 19.99 },
      ]),
    );
  });

  afterEach(() => {
    rmSync(dir, { recursive: true, force: true });
  });

  it("creates an instance and queries data", () => {
    const db = new DirSQL(dir, [
      {
        ddl: "CREATE TABLE users (name TEXT, age INTEGER)",
        glob: "data/users.json",
        extract: (_filePath: string, content: string) => JSON.parse(content),
      },
    ]);

    const rows = db.query("SELECT * FROM users ORDER BY name");
    expect(rows).toHaveLength(2);
    expect(rows[0].name).toBe("Alice");
    expect(rows[0].age).toBe(30);
    expect(rows[1].name).toBe("Bob");
    expect(rows[1].age).toBe(25);
  });

  it("supports multiple tables", () => {
    const db = new DirSQL(dir, [
      {
        ddl: "CREATE TABLE users (name TEXT, age INTEGER)",
        glob: "data/users.json",
        extract: (_filePath: string, content: string) => JSON.parse(content),
      },
      {
        ddl: "CREATE TABLE products (name TEXT, price REAL)",
        glob: "data/products.json",
        extract: (_filePath: string, content: string) => JSON.parse(content),
      },
    ]);

    const users = db.query("SELECT * FROM users ORDER BY name");
    expect(users).toHaveLength(2);

    const products = db.query("SELECT * FROM products ORDER BY name");
    expect(products).toHaveLength(2);
    expect(products[0].name).toBe("Gadget");
    expect(products[0].price).toBeCloseTo(19.99);
  });

  it("supports glob patterns", () => {
    const db = new DirSQL(dir, [
      {
        ddl: "CREATE TABLE items (name TEXT)",
        glob: "data/*.json",
        extract: (_filePath: string, content: string) =>
          JSON.parse(content).map((item: { name: string }) => ({
            name: item.name,
          })),
      },
    ]);

    const rows = db.query("SELECT * FROM items ORDER BY name");
    expect(rows).toHaveLength(4);
  });

  it("supports ignore patterns", () => {
    const db = new DirSQL(
      dir,
      [
        {
          ddl: "CREATE TABLE items (name TEXT)",
          glob: "data/*.json",
          extract: (_filePath: string, content: string) =>
            JSON.parse(content).map((item: { name: string }) => ({
              name: item.name,
            })),
        },
      ],
      ["data/products.json"],
    );

    const rows = db.query("SELECT * FROM items ORDER BY name");
    expect(rows).toHaveLength(2);
  });

  it("handles SQL queries with WHERE clauses", () => {
    const db = new DirSQL(dir, [
      {
        ddl: "CREATE TABLE users (name TEXT, age INTEGER)",
        glob: "data/users.json",
        extract: (_filePath: string, content: string) => JSON.parse(content),
      },
    ]);

    const rows = db.query("SELECT * FROM users WHERE age > 27");
    expect(rows).toHaveLength(1);
    expect(rows[0].name).toBe("Alice");
  });

  it("handles empty directories gracefully", () => {
    const emptyDir = mkdtempSync(join(tmpdir(), "dirsql-empty-"));
    try {
      const db = new DirSQL(emptyDir, [
        {
          ddl: "CREATE TABLE items (name TEXT)",
          glob: "**/*.json",
          extract: (_filePath: string, content: string) => JSON.parse(content),
        },
      ]);

      const rows = db.query("SELECT * FROM items");
      expect(rows).toHaveLength(0);
    } finally {
      rmSync(emptyDir, { recursive: true, force: true });
    }
  });

  it("throws on invalid SQL", () => {
    const db = new DirSQL(dir, [
      {
        ddl: "CREATE TABLE users (name TEXT)",
        glob: "data/users.json",
        extract: (_filePath: string, content: string) => JSON.parse(content),
      },
    ]);

    expect(() => db.query("SELECT * FROM nonexistent")).toThrow();
  });

  it("throws on invalid DDL", () => {
    expect(
      () =>
        new DirSQL(dir, [
          {
            ddl: "NOT VALID SQL",
            glob: "**/*.json",
            extract: () => [],
          },
        ]),
    ).toThrow();
  });
});
