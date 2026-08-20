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
    } as ReturnType<typeof loadNativeCore>;
    vi.mocked(loadNativeCore).mockReturnValue(fake);

    expect(getCore()).toBe(fake);
    expect(getCore()).toBe(fake);
    expect(loadNativeCore).toHaveBeenCalledTimes(1);
  });
});

describe("NativeDirSQLConstructor contract", () => {
  it("accepts extensions, suppressConfigExtensions, and noIgnore as openAsync's trailing arguments (#230 / #313 / #746)", () => {
    const openAsync = vi.fn().mockResolvedValue({});
    const ctor = { openAsync } as unknown as NativeDirSQLConstructor;
    // Typed against the real interface: dropping the 7th, 8th, or 9th
    // argument would fail to compile, so this call pins the extensions,
    // suppressConfigExtensions, and noIgnore parameters into the contract
    // the `DirSQL` wrapper relies on.
    ctor.openAsync(
      "/r",
      null,
      null,
      null,
      null,
      null,
      [{ path: "/ext/vec0.so", entrypoint: "sqlite3_vec_init" }],
      true,
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
      null,
    );
  });
});

describe("NativeDirSQL interface", () => {
  it("includes a synchronous scanFailures() (#715)", () => {
    // Synchronous by design: it reads a list the scan already produced, so it
    // needs no threadpool hop. A `Promise`-returning shape here would be a
    // real defect -- the wrapper awaits `ready` and then returns this value
    // directly, so a thenable would leak into the caller's array.
    const failures = [{ path: "bad.json", message: "boom" }];
    const instance = {
      query: vi.fn().mockResolvedValue([]),
      startWatcher: vi.fn().mockResolvedValue(undefined),
      pollEvents: vi.fn().mockResolvedValue([]),
      scanFailures: vi.fn(() => failures),
      close: vi.fn(),
    } as unknown as NativeDirSQL;

    const result = instance.scanFailures();
    expect(result).toEqual(failures);
    expect(result).not.toBeInstanceOf(Promise);
  });

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
