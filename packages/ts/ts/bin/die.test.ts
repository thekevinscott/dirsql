import { afterEach, describe, expect, it, vi } from "vitest";
import { die } from "./die.js";

afterEach(() => vi.restoreAllMocks());

describe("die", () => {
  describe("by default", () => {
    it("writes a `dirsql: ` prefixed message to stderr", () => {
      const write = vi.spyOn(process.stderr, "write").mockReturnValue(true);
      vi.spyOn(process, "exit").mockImplementation(((
        code?: string | number | null,
      ) => {
        throw new Error(`exit:${code}`);
      }) as typeof process.exit);

      expect(() => die("boom")).toThrow("exit:1");
      expect(write).toHaveBeenCalledWith("dirsql: boom\n");
    });

    it("exits with code 1", () => {
      vi.spyOn(process.stderr, "write").mockReturnValue(true);
      const exit = vi.spyOn(process, "exit").mockImplementation(((
        code?: string | number | null,
      ) => {
        throw new Error(`exit:${code}`);
      }) as typeof process.exit);

      expect(() => die("boom")).toThrow();
      expect(exit).toHaveBeenCalledWith(1);
    });
  });

  describe("with an explicit exit code", () => {
    it("passes the code to process.exit", () => {
      vi.spyOn(process.stderr, "write").mockReturnValue(true);
      const exit = vi.spyOn(process, "exit").mockImplementation(((
        code?: string | number | null,
      ) => {
        throw new Error(`exit:${code}`);
      }) as typeof process.exit);

      expect(() => die("nope", 7)).toThrow();
      expect(exit).toHaveBeenCalledWith(7);
    });
  });
});
