import { afterEach, describe, expect, it, vi } from "vitest";
import { hasBareName } from "./has-bare-name.js";
import { isBareName } from "./is-bare-name.js";

vi.mock("./is-bare-name.js", async () => ({
  ...(await vi.importActual<typeof import("./is-bare-name.js")>(
    "./is-bare-name.js",
  )),
  isBareName: vi.fn(),
}));

afterEach(() => vi.resetAllMocks());

describe("hasBareName", () => {
  it("is false for no entries", () => {
    expect(hasBareName([])).toBe(false);
    expect(isBareName).not.toHaveBeenCalled();
  });

  it("asks isBareName about each string path", () => {
    vi.mocked(isBareName).mockReturnValue(false);
    expect(hasBareName([{ path: "ext/a.so" }, { path: "ext/b.so" }])).toBe(
      false,
    );
    expect(vi.mocked(isBareName).mock.calls).toEqual([
      ["ext/a.so"],
      ["ext/b.so"],
    ]);
  });

  it("is true when some entry is a bare name", () => {
    vi.mocked(isBareName).mockImplementation((p) => p === "sqlite_vec");
    expect(hasBareName([{ path: "ext/a.so" }, { path: "sqlite_vec" }])).toBe(
      true,
    );
  });

  it("skips an entry whose path is not a string", () => {
    vi.mocked(isBareName).mockReturnValue(true);
    expect(hasBareName([{ path: 42 }])).toBe(false);
    expect(isBareName).not.toHaveBeenCalled();
  });
});
