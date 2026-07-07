import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { die } from "./die.js";
import { defaultResolver, resolveBinary } from "./resolve-binary.js";

vi.mock("./die.js");

describe("resolveBinary", () => {
  beforeEach(() => {
    vi.mocked(die).mockImplementation(((msg: string) => {
      throw new Error(`DIE: ${msg}`);
    }) as typeof die);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.resetAllMocks();
  });

  describe("when the host triple is unknown", () => {
    it("calls die with a 'no prebuilt binary' message", () => {
      expect(() => resolveBinary("unknown-triple", () => "ignored")).toThrow(
        /^DIE: no prebuilt binary for unknown-triple\./,
      );
    });
  });

  describe("when the host triple is known", () => {
    beforeEach(() => {
      vi.stubGlobal("process", {
        ...process,
        platform: "linux",
        arch: "x64",
      });
    });

    it("resolves the binary via the injected resolver", () => {
      const resolver = vi.fn().mockReturnValue("/abs/path/to/dirsql");

      const result = resolveBinary(undefined, resolver);

      expect(result).toBe("/abs/path/to/dirsql");
      expect(resolver).toHaveBeenCalledWith("@dirsql/cli-linux-x64-gnu/dirsql");
    });

    it("uses `dirsql.exe` on win32", () => {
      vi.stubGlobal("process", { ...process, platform: "win32", arch: "x64" });
      const resolver = vi.fn().mockReturnValue("C:/dirsql.exe");

      resolveBinary(undefined, resolver);

      expect(resolver).toHaveBeenCalledWith(
        "@dirsql/cli-win32-x64-msvc/dirsql.exe",
      );
    });

    it("calls die with a 'not installed' message when the resolver throws", () => {
      const resolver = () => {
        throw new Error("MODULE_NOT_FOUND");
      };

      expect(() => resolveBinary(undefined, resolver)).toThrow(
        /^DIE: @dirsql\/cli-linux-x64-gnu is not installed\./,
      );
    });
  });
});

describe("defaultResolver", () => {
  it("returns a require.resolve-shaped function", () => {
    expect(typeof defaultResolver()).toBe("function");
  });
});
