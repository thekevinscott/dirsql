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

  it("routes `new DirSQL(...)` through the injected fake core", async () => {
    const fakeInstance = {
      query: vi.fn(async () => [{ injected: true }]),
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
    expect(await db.query("SELECT 1")).toEqual([{ injected: true }]);
    expect(fakeInstance.query).toHaveBeenCalledWith("SELECT 1");
  });

  it("routes static methods (fromConfig) through the injected fake core", async () => {
    const fromConfig = vi.fn(() => ({
      query: async () => [{ via: "fromConfig" }],
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
    expect(await db.query("x")).toEqual([{ via: "fromConfig" }]);
  });

  it("exposes watch() as an AsyncIterable driven by pollEvents", async () => {
    const queued: dirsql.RowEvent[][] = [
      [],
      [
        {
          table: "items",
          action: "insert",
          row: { name: "one" },
          filePath: "a.json",
        },
      ],
      [
        {
          table: "items",
          action: "insert",
          row: { name: "two" },
          filePath: "b.json",
        },
      ],
    ];
    const fakeInstance = {
      query: async () => [],
      startWatcher: vi.fn(),
      pollEvents: vi.fn(() => queued.shift() ?? []),
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

    const db = new dirsql.DirSQL("/tmp/nothing", []);
    const seen: dirsql.RowEvent[] = [];
    for await (const event of db.watch()) {
      seen.push(event);
      if (seen.length >= 2) break;
    }

    expect(fakeInstance.startWatcher).toHaveBeenCalledTimes(1);
    expect(seen).toHaveLength(2);
    expect(seen[0].row).toEqual({ name: "one" });
    expect(seen[1].row).toEqual({ name: "two" });
  });

  // Regression test for https://github.com/thekevinscott/dirsql/issues/119:
  // watch() called pollEvents(200) in a `while(true)` loop with no `await`
  // between iterations. A synchronous napi call plus an empty event batch
  // means the JS event loop never gets a chance to run between polls —
  // same-process setTimeout callbacks, microtasks, and fs writes are all
  // starved. The fix uses a non-blocking poll timeout and yields to the
  // event loop between iterations.
  it("yields to the event loop between polls so same-process timers fire", async () => {
    let callCount = 0;
    // Synchronously returning [] from pollEvents under the old impl caused
    // an unbounded sync busy-loop. Bound it here so the test terminates
    // cleanly either way.
    const pollEvents = vi.fn(() => {
      callCount += 1;
      if (callCount > 200) {
        throw new Error(
          "watch() did not yield to the event loop between polls",
        );
      }
      return [];
    });
    const fakeInstance = {
      query: async () => [],
      startWatcher: vi.fn(),
      pollEvents,
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

    const db = new dirsql.DirSQL("/tmp/nothing", []);

    let timerFired = false;
    setTimeout(() => {
      timerFired = true;
    }, 5);

    const iter = db.watch()[Symbol.asyncIterator]();
    // Start the generator's poll loop. Don't await — with the broken
    // implementation this promise never resolves because pollEvents
    // always returns [] and the generator never hits `yield`.
    const pending = iter.next();
    pending.catch(() => {
      /* swallow — we force-terminate via iter.return below */
    });

    // Give the timer a generous window to fire. With the old 200ms
    // blocking poll (or the sync busy-loop with an empty batch), no
    // setTimeout callback whose delay is < 200ms will fire in time.
    await new Promise<void>((resolve) => setTimeout(resolve, 60));
    await iter.return?.();

    expect(timerFired).toBe(true);
    expect(pollEvents).toHaveBeenCalled();
    for (const [timeoutMs] of pollEvents.mock.calls as [number][]) {
      // Each native poll must not block long enough to starve the loop.
      expect(timeoutMs).toBeLessThan(100);
    }
  });
});
