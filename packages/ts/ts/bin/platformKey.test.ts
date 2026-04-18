import { afterEach, describe, expect, it, vi } from "vitest";
import { platformKey } from "./platformKey.js";

afterEach(() => vi.restoreAllMocks());

describe("platformKey", () => {
  describe("on linux x64", () => {
    it("returns 'linux-x64'", () => {
      vi.stubGlobal("process", {
        ...process,
        platform: "linux",
        arch: "x64",
      });
      expect(platformKey()).toBe("linux-x64");
    });
  });

  describe("on darwin arm64", () => {
    it("returns 'darwin-arm64'", () => {
      vi.stubGlobal("process", {
        ...process,
        platform: "darwin",
        arch: "arm64",
      });
      expect(platformKey()).toBe("darwin-arm64");
    });
  });

  describe("on win32 x64", () => {
    it("returns 'win32-x64'", () => {
      vi.stubGlobal("process", {
        ...process,
        platform: "win32",
        arch: "x64",
      });
      expect(platformKey()).toBe("win32-x64");
    });
  });
});
