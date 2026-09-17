import { readFileSync } from "node:fs";
import { afterEach, describe, expect, it, vi } from "vitest";
import { readConfig } from "./read-config.js";

vi.mock("node:fs", async () => ({
  ...(await vi.importActual<typeof import("node:fs")>("node:fs")),
  readFileSync: vi.fn(),
}));

afterEach(() => vi.resetAllMocks());

describe("readConfig", () => {
  it("returns the config's text", () => {
    vi.mocked(readFileSync).mockReturnValue("name = 1\n");
    expect(readConfig("/a/.dirsql.toml")).toBe("name = 1\n");
    expect(readFileSync).toHaveBeenCalledWith("/a/.dirsql.toml", "utf8");
  });

  it("returns undefined when the config cannot be read", () => {
    vi.mocked(readFileSync).mockImplementation(() => {
      throw new Error("ENOENT");
    });
    expect(readConfig("/gone/.dirsql.toml")).toBeUndefined();
  });
});
