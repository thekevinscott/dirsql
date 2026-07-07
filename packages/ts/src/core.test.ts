import { describe, expect, it, vi } from "vitest";
import { type NativeDirSQLConstructor, getCore } from "./core.js";
import { loadNativeCore } from "./load-native-core.js";

vi.mock("./load-native-core.js");

describe("getCore", () => {
  it("lazily loads the native core on first access and caches it", () => {
    const fake = {
      DirSQL: {},
      parseTableName: () => null,
    } as ReturnType<typeof loadNativeCore>;
    vi.mocked(loadNativeCore).mockReturnValue(fake);

    expect(getCore()).toBe(fake);
    expect(getCore()).toBe(fake);
    expect(loadNativeCore).toHaveBeenCalledTimes(1);
  });
});

describe("NativeDirSQLConstructor contract", () => {
  it("accepts extensions and suppressConfigExtensions as openAsync's trailing arguments (#230 / #313)", () => {
    const openAsync = vi.fn().mockResolvedValue({});
    const ctor = { openAsync } as unknown as NativeDirSQLConstructor;
    // Typed against the real interface: dropping the 7th or 8th argument
    // would fail to compile, so this call pins the extensions and
    // suppressConfigExtensions parameters into the contract the `DirSQL`
    // wrapper relies on.
    ctor.openAsync(
      "/r",
      null,
      null,
      null,
      null,
      null,
      [{ path: "/ext/vec0.so", entrypoint: "sqlite3_vec_init" }],
      true,
    );
    expect(openAsync).toHaveBeenCalledWith(
      "/r",
      null,
      null,
      null,
      null,
      null,
      [{ path: "/ext/vec0.so", entrypoint: "sqlite3_vec_init" }],
      true,
    );
  });
});
