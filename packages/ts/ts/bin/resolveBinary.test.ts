import { afterEach, describe, expect, it, vi } from "vitest";
import {
  type Resolver,
  defaultResolver,
  resolveBinary,
} from "./resolveBinary.js";

afterEach(() => {
  vi.restoreAllMocks();
});

function exitThrower() {
  return vi.spyOn(process, "exit").mockImplementation(((
    code?: string | number | null,
  ) => {
    throw new Error(`exit:${code}`);
  }) as typeof process.exit);
}

describe("defaultResolver", () => {
  it("returns a function that resolves node built-ins", () => {
    const resolve = defaultResolver();
    expect(typeof resolve).toBe("function");
    // `node:fs` is always resolvable from any Node-reachable module.
    expect(resolve("node:fs")).toBe("node:fs");
  });
});

describe("resolveBinary", () => {
  describe("with a supported platform key", () => {
    it("returns the resolver's result for the matching package", () => {
      const resolver = vi.fn<Resolver>().mockReturnValue("/real/linux/dirsql");
      const path = resolveBinary("linux-x64", resolver);
      expect(path).toBe("/real/linux/dirsql");
      expect(resolver).toHaveBeenCalledWith(
        expect.stringContaining("@dirsql/cli-linux-x64-gnu"),
      );
    });
  });

  describe("with an unsupported platform key", () => {
    it("dies with a `cargo install` hint", () => {
      vi.spyOn(process.stderr, "write").mockReturnValue(true);
      const exit = exitThrower();
      expect(() => resolveBinary("sunos-sparc", () => "")).toThrow();
      expect(exit).toHaveBeenCalledWith(1);
      expect(process.stderr.write).toHaveBeenCalledWith(
        expect.stringContaining("no prebuilt binary for sunos-sparc"),
      );
    });
  });

  describe("on Windows", () => {
    it("appends .exe to the resolved binary name", () => {
      vi.stubGlobal("process", { ...process, platform: "win32" });
      const resolver = vi
        .fn<Resolver>()
        .mockReturnValue("C:\\fake\\dirsql.exe");
      resolveBinary("win32-x64", resolver);
      expect(resolver).toHaveBeenCalledWith(
        expect.stringMatching(/dirsql\.exe$/),
      );
    });
  });

  describe("when the optional dep is not installed", () => {
    it("dies with an `--no-optional` hint", () => {
      vi.spyOn(process.stderr, "write").mockReturnValue(true);
      const exit = exitThrower();
      const resolver: Resolver = () => {
        throw new Error("Cannot find module");
      };
      expect(() => resolveBinary("linux-x64", resolver)).toThrow();
      expect(exit).toHaveBeenCalledWith(1);
      expect(process.stderr.write).toHaveBeenCalledWith(
        expect.stringContaining("is not installed"),
      );
    });
  });
});
