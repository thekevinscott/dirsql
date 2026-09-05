import { describe, expect, it } from "vitest";
import { defaultResolver } from "./default-resolver.js";

describe("defaultResolver", () => {
  it("resolves a real module through require.resolve", () => {
    expect(defaultResolver().resolve("vitest")).toContain("vitest");
  });

  it("throws for a module that is not installed", () => {
    expect(() =>
      defaultResolver().resolve("dirsql-nonexistent-pkg-xyz"),
    ).toThrow(/Cannot find module/);
  });

  it("reports the require.resolve search paths", () => {
    const paths = defaultResolver().paths("vitest");
    expect(paths).toEqual(expect.any(Array));
    expect(paths?.length).toBeGreaterThan(0);
  });
});
