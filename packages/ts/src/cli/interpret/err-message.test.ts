import { describe, expect, it } from "vitest";
import { errMessage } from "./err-message.js";

describe("errMessage", () => {
  it("returns Error.message when the value is an Error", () => {
    expect(errMessage(new Error("boom"))).toBe("boom");
  });

  it("returns Error.message for a subclassed Error", () => {
    expect(errMessage(new TypeError("bad type"))).toBe("bad type");
  });

  it("coerces a string to itself", () => {
    expect(errMessage("plain")).toBe("plain");
  });

  it("coerces null to 'null'", () => {
    expect(errMessage(null)).toBe("null");
  });

  it("coerces a number via String()", () => {
    expect(errMessage(42)).toBe("42");
  });

  it("coerces a plain object via String()", () => {
    expect(errMessage({})).toBe("[object Object]");
  });
});
