import { describe, expect, it } from "vitest";
import { Table } from "./table.js";

describe("Table", () => {
  const extract = () => [{ a: 1 }];

  it("copies ddl, glob, and extract onto the instance", () => {
    const t = new Table({
      ddl: "CREATE TABLE t (a INTEGER)",
      glob: "*.json",
      extract,
    });
    expect(t.ddl).toBe("CREATE TABLE t (a INTEGER)");
    expect(t.glob).toBe("*.json");
    expect(t.extract).toBe(extract);
  });

  it("copies strict when present", () => {
    const t = new Table({ ddl: "d", glob: "g", extract, strict: true });
    expect(t.strict).toBe(true);
    expect(Object.keys(t).sort()).toEqual(["ddl", "extract", "glob", "strict"]);
  });

  it("omits strict from enumerable keys when absent", () => {
    const t = new Table({ ddl: "d", glob: "g", extract });
    expect(t.strict).toBeUndefined();
    expect(Object.keys(t).sort()).toEqual(["ddl", "extract", "glob"]);
  });
});
