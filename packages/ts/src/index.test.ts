import { describe, expect, it } from "vitest";
import * as api from "./index.js";
import type { QueryResult } from "./index.js";

describe("public barrel", () => {
  it("re-exports the runtime values", () => {
    expect(typeof api.DirSQL).toBe("function");
    expect(typeof api.Table).toBe("function");
  });

  it("re-exports the QueryResult type with its documented shape", () => {
    const result: QueryResult = { columns: ["a"], rows: [{ a: 1 }] };
    expect(result.columns).toEqual(["a"]);
    expect(result.rows).toEqual([{ a: 1 }]);
  });

  it("exposes exactly the public runtime exports", () => {
    expect(Object.keys(api).sort()).toEqual(["DirSQL", "Table"].sort());
  });
});
