import { describe, expect, it } from "vitest";
import { insertFlags } from "./insert-flags.js";

describe("insertFlags", () => {
  it("appends when argv has no end-of-options marker", () => {
    expect(insertFlags(["query", "x"], ["-c", "f"])).toEqual([
      "query",
      "x",
      "-c",
      "f",
    ]);
  });

  it("inserts before the first end-of-options marker", () => {
    expect(
      insertFlags(["--format", "json", "--", "x", "--"], ["-c", "f"]),
    ).toEqual(["--format", "json", "-c", "f", "--", "x", "--"]);
  });

  it("inserts before a leading marker", () => {
    expect(insertFlags(["--", "x"], ["-c", "f"])).toEqual([
      "-c",
      "f",
      "--",
      "x",
    ]);
  });
});
