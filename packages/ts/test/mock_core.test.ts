// Verifies the test-only boundary `__setCoreForTesting` installed in
// `ts/index.ts`. This proves vitest can swap in a fake core module so
// binding-layer tests don't need the real napi-rs binary.
//
// The stub-file approach tried in a prior PR failed in CI because the
// real `dirsql.node` shadowed the fake at require-time. This boundary
// test exercises the lazy-factory path instead.

import * as dirsql from "dirsql";
import { afterEach, describe, expect, it, vi } from "vitest";

describe("__setCoreForTesting", () => {
  afterEach(() => {
    // Reset to the real native core after each test.
    (
      dirsql as unknown as {
        __setCoreForTesting: (c: unknown) => void;
      }
    ).__setCoreForTesting(null);
  });

  it("routes `new DirSQL(...)` through the injected fake core", () => {
    const fakeInstance = {
      query: vi.fn(() => [{ injected: true }]),
      startWatcher: vi.fn(),
      pollEvents: vi.fn(() => []),
    };
    const FakeDirSQL = vi.fn(function (this: unknown) {
      return fakeInstance;
    }) as unknown as new (
      ...args: unknown[]
    ) => typeof fakeInstance;

    (
      dirsql as unknown as {
        __setCoreForTesting: (c: { DirSQL: unknown }) => void;
      }
    ).__setCoreForTesting({ DirSQL: FakeDirSQL });

    const db = new dirsql.DirSQL("/tmp/does-not-exist", []);
    expect(FakeDirSQL).toHaveBeenCalledWith("/tmp/does-not-exist", []);
    expect(db.query("SELECT 1")).toEqual([{ injected: true }]);
    expect(fakeInstance.query).toHaveBeenCalledWith("SELECT 1");
  });

  it("routes static methods (fromConfig) through the injected fake core", () => {
    const fromConfig = vi.fn(() => ({
      query: () => [{ via: "fromConfig" }],
      startWatcher: () => {},
      pollEvents: () => [],
    }));
    const FakeDirSQL = Object.assign(
      () => {
        throw new Error("ctor not expected");
      },
      { fromConfig },
    );

    (
      dirsql as unknown as {
        __setCoreForTesting: (c: { DirSQL: unknown }) => void;
      }
    ).__setCoreForTesting({ DirSQL: FakeDirSQL });

    const db = dirsql.DirSQL.fromConfig("/tmp/fake.toml");
    expect(fromConfig).toHaveBeenCalledWith("/tmp/fake.toml");
    expect(db.query("x")).toEqual([{ via: "fromConfig" }]);
  });
});
