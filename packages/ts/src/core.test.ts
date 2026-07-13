import { describe, expect, it, vi } from "vitest";
import {
  type NativeDirSQL,
  type NativeDirSQLConstructor,
  getCore,
} from "./core.js";
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

  it("accepts an array of config paths as openAsync's fourth argument (#589)", () => {
    const openAsync = vi.fn().mockResolvedValue({});
    const ctor = { openAsync } as unknown as NativeDirSQLConstructor;
    // Pins the repeatable-config contract: `config` is `string[] | null`, so
    // passing a single string here would fail to compile.
    ctor.openAsync(
      "/r",
      null,
      null,
      ["/a.toml", "/b.toml"],
      null,
      null,
      null,
      null,
    );
    expect(openAsync).toHaveBeenCalledWith(
      "/r",
      null,
      null,
      ["/a.toml", "/b.toml"],
      null,
      null,
      null,
      null,
    );
  });
});

describe("NativeDirSQL interface", () => {
  it("includes close() method for cleanup (#598)", () => {
    const instance = {
      query: vi.fn().mockResolvedValue([]),
      startWatcher: vi.fn().mockResolvedValue(undefined),
      pollEvents: vi.fn().mockResolvedValue([]),
      close: vi.fn(),
    } as unknown as NativeDirSQL;
    // Verifies the interface has the close() method; calling it exercises
    // that the method exists and is callable.
    instance.close();
    expect(instance.close).toHaveBeenCalledOnce();
  });
});
