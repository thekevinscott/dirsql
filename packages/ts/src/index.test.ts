// Coverage for the public barrel: assert it re-exports the SDK's public
// surface from the colocated-tested implementation modules. Previously the
// barrel was executed only incidentally (the deleted `cli/interpret`
// modules imported it); after the #324 interpret removal it needs its own
// importer so the re-export statements stay covered.

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
