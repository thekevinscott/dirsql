import { describe, expect, it } from "vitest";
import { Table } from "./table.js";

describe("Table", () => {
  const onFile = () => [{ a: 1 }];

  it("copies ddl, glob, and onFile onto the instance", () => {
    const t = new Table({
      ddl: "CREATE TABLE t (a INTEGER)",
      glob: "*.json",
      onFile,
    });
    expect(t.ddl).toBe("CREATE TABLE t (a INTEGER)");
    expect(t.glob).toBe("*.json");
    expect(t.onFile).toBe(onFile);
  });

  it("copies strict when present", () => {
    const t = new Table({ ddl: "d", glob: "g", onFile, strict: true });
    expect(t.strict).toBe(true);
    expect(Object.keys(t).sort()).toEqual(["ddl", "glob", "onFile", "strict"]);
  });

  it("omits strict from enumerable keys when absent", () => {
    const t = new Table({ ddl: "d", glob: "g", onFile });
    expect(t.strict).toBeUndefined();
    expect(Object.keys(t).sort()).toEqual(["ddl", "glob", "onFile"]);
  });
});
