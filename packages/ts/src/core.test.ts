import { afterEach, describe, expect, it, vi } from "vitest";
import { __setCoreForTesting, getCore } from "./core.js";
import { loadNativeCore } from "./load-native-core.js";

vi.mock("./load-native-core.js");

describe("core", () => {
  afterEach(() => {
    __setCoreForTesting(null);
    vi.resetAllMocks();
  });

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

  it("returns the injected fake without touching the native loader", () => {
    const fake = {
      DirSQL: {},
      parseTableName: () => "t",
    } as unknown as Parameters<typeof __setCoreForTesting>[0];
    __setCoreForTesting(fake);

    expect(getCore()).toBe(fake);
    expect(loadNativeCore).not.toHaveBeenCalled();
  });

  it("resets to the lazy native load when passed null", () => {
    const injected = {
      DirSQL: {},
      parseTableName: () => null,
    } as unknown as Parameters<typeof __setCoreForTesting>[0];
    __setCoreForTesting(injected);
    expect(getCore()).toBe(injected);

    __setCoreForTesting(null);
    const native = {
      DirSQL: {},
      parseTableName: () => null,
    } as ReturnType<typeof loadNativeCore>;
    vi.mocked(loadNativeCore).mockReturnValue(native);
    expect(getCore()).toBe(native);
  });
});
