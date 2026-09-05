import { describe, expect, it } from "vitest";
import { isBareName } from "./is-bare-name.js";

describe("isBareName", () => {
  it("treats a separatorful value as a path", () => {
    expect(isBareName("ext/vec0.so")).toBe(false);
    expect(isBareName("C:\\ext\\vec0.dll")).toBe(false);
  });

  it("treats a forward slash alone as a path", () => {
    expect(isBareName("scope/pkg")).toBe(false);
  });

  it("treats a backslash alone as a path", () => {
    expect(isBareName("a\\b")).toBe(false);
  });

  it("treats a loadable suffix as a path", () => {
    expect(isBareName("vec0.so")).toBe(false);
    expect(isBareName("vec0.dylib")).toBe(false);
    expect(isBareName("vec0.dll")).toBe(false);
    expect(isBareName("vec0.node")).toBe(false);
  });

  it("treats a plain identifier as a bare name", () => {
    expect(isBareName("sqlite-vec")).toBe(true);
  });

  it("only matches a loadable suffix at the end", () => {
    expect(isBareName("so-package")).toBe(true);
    expect(isBareName("node-thing")).toBe(true);
  });
});
