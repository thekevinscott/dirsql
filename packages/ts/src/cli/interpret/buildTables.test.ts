import { describe, expect, it, vi } from "vitest";
import type { DirSQL, TableDef } from "../../index.js";
import { buildTables } from "./buildTables.js";

// `buildTables`'s only collaborator is `parseTableName` from the core barrel.
// Mock it so the SUT picks up the fake directly -- no real core, no
// `__setCoreForTesting` seam.
// Anchor the double to the real barrel (so it can't drift), then override the
// only collaborator `buildTables` reaches for. The barrel's `parseTableName`
// lazy-loads the native core *when called*; the fake below is what runs, so
// the real native implementation is never invoked.
vi.mock("../../index.js", async () => ({
  ...(await vi.importActual<typeof import("../../index.js")>("../../index.js")),
  parseTableName: vi.fn(
    (ddl: string) => /CREATE\s+TABLE\s+(\w+)/i.exec(ddl)?.[1] ?? null,
  ),
}));

const noopExtract: TableDef["extract"] = () => [];

function fakeApp(tables: TableDef[] | undefined): DirSQL {
  return { _options: { tables } } as unknown as DirSQL;
}

describe("buildTables", () => {
  it("returns an empty map when the app has no tables", () => {
    expect(buildTables(fakeApp([]))).toEqual(new Map());
  });

  it("treats undefined tables as empty", () => {
    expect(buildTables(fakeApp(undefined))).toEqual(new Map());
  });

  it("keys the lookup by the SQL identifier parsed from each DDL", () => {
    const t: TableDef = {
      ddl: "CREATE TABLE papers (title TEXT)",
      glob: "**/*.json",
      extract: noopExtract,
    };
    const tables = buildTables(fakeApp([t]));
    expect(tables.get("papers")).toBe(t);
  });

  it("preserves multiple tables under their respective names", () => {
    const a: TableDef = {
      ddl: "CREATE TABLE a (x TEXT)",
      glob: "a/*",
      extract: noopExtract,
    };
    const b: TableDef = {
      ddl: "CREATE TABLE b (y TEXT)",
      glob: "b/*",
      extract: noopExtract,
    };
    const tables = buildTables(fakeApp([a, b]));
    expect(tables.get("a")).toBe(a);
    expect(tables.get("b")).toBe(b);
    expect(tables.size).toBe(2);
  });

  it("throws when parseTableName returns null for a DDL", () => {
    const t: TableDef = {
      // DDL the fake regex won't match -- no `CREATE TABLE` prefix at all.
      ddl: "DROP TABLE old_papers",
      glob: "*.json",
      extract: noopExtract,
    };
    expect(() => buildTables(fakeApp([t]))).toThrow(
      /could not parse table name/,
    );
  });
});
