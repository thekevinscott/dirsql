import { afterEach, describe, expect, it } from "vitest";
import { platformSuffixes } from "./platform-suffixes.js";

const original = process.platform;

function pinPlatform(value: NodeJS.Platform) {
  Object.defineProperty(process, "platform", { value, configurable: true });
}

describe("platformSuffixes", () => {
  afterEach(() =>
    Object.defineProperty(process, "platform", {
      value: original,
      configurable: true,
    }),
  );

  it("globs dylib and node on macOS", () => {
    pinPlatform("darwin");
    expect(platformSuffixes()).toEqual([".dylib", ".node"]);
  });

  it("globs dll and node on windows", () => {
    pinPlatform("win32");
    expect(platformSuffixes()).toEqual([".dll", ".node"]);
  });

  it("globs so and node everywhere else", () => {
    pinPlatform("linux");
    expect(platformSuffixes()).toEqual([".so", ".node"]);
    pinPlatform("freebsd");
    expect(platformSuffixes()).toEqual([".so", ".node"]);
  });
});
