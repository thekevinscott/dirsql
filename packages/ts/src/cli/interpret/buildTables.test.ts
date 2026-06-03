import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  type DirSQL,
  type TableDef,
  __setCoreForTesting,
} from "../../index.js";
import { buildTables } from "./buildTables.js";

const noopExtract: TableDef["extract"] = () => [];

function fakeApp(tables: TableDef[] | undefined): DirSQL {
  return { _options: { tables } } as unknown as DirSQL;
}

describe("buildTables", () => {
  beforeEach(() => {
    __setCoreForTesting({
      // biome-ignore lint/suspicious/noExplicitAny: minimal core stub
      DirSQL: {} as any,
      parseTableName: vi
        .fn()
        .mockImplementation(
          (ddl: string) => /CREATE\s+TABLE\s+(\w+)/i.exec(ddl)?.[1] ?? null,
        ),
    });
  });

  afterEach(() => {
    __setCoreForTesting(null);
  });

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
      ddl: "this is not a CREATE TABLE statement",
      glob: "*.json",
      extract: noopExtract,
    };
    expect(() => buildTables(fakeApp([t]))).toThrow(
      /could not parse table name/,
    );
  });
});
