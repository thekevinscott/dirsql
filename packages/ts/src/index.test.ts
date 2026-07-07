import { describe, expect, it } from "vitest";
import * as api from "./index.js";

describe("public barrel", () => {
  it("re-exports the runtime values", () => {
    expect(typeof api.DirSQL).toBe("function");
    expect(typeof api.Table).toBe("function");
    expect(typeof api.parseTableName).toBe("function");
  });

  it("exposes exactly the public runtime exports", () => {
    expect(Object.keys(api).sort()).toEqual(
      ["DirSQL", "Table", "parseTableName"].sort(),
    );
  });
});
