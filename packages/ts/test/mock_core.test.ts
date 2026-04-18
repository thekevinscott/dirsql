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
      startWatcher: vi.fn(async () => {}),
      pollEvents: vi.fn(async () => []),
    };
    const openAsync = vi.fn(async () => fakeInstance);
    // The ctor is unused on the async path — assert it isn't called.
    const FakeDirSQL = Object.assign(
      vi.fn(function (this: unknown) {
        throw new Error("sync ctor should not be called");
      }),
      { openAsync },
    ) as unknown as (new (
      ...args: unknown[]
    ) => typeof fakeInstance) & {
      openAsync: typeof openAsync;
    };

    (
      dirsql as unknown as {
        __setCoreForTesting: (c: { DirSQL: unknown }) => void;
      }
    ).__setCoreForTesting({ DirSQL: FakeDirSQL });

    const db = new dirsql.DirSQL({ root: "/tmp/does-not-exist", tables: [] });
    await db.ready;
    expect(openAsync).toHaveBeenCalledWith(
      "/tmp/does-not-exist",
      [],
      null,
      null,
    );
    expect(await db.query("SELECT 1")).toEqual([{ injected: true }]);
    expect(fakeInstance.query).toHaveBeenCalledWith("SELECT 1");
  });

  it("routes `new DirSQL(configPath)` through the injected fake core", async () => {
    const fakeInstance = {
      query: vi.fn(async () => [{ via: "config" }]),
      startWatcher: vi.fn(async () => {}),
      pollEvents: vi.fn(async () => []),
    };
    const openAsync = vi.fn(async () => fakeInstance);
    const FakeDirSQL = Object.assign(
      vi.fn(function (this: unknown) {
        throw new Error("sync ctor should not be called");
      }),
      { openAsync },
    );

    (
      dirsql as unknown as {
        __setCoreForTesting: (c: { DirSQL: unknown }) => void;
      }
    ).__setCoreForTesting({ DirSQL: FakeDirSQL });

    const db = new dirsql.DirSQL("/tmp/fake.toml");
    await db.ready;
    expect(openAsync).toHaveBeenCalledWith(null, null, null, "/tmp/fake.toml");
    expect(await db.query("x")).toEqual([{ via: "config" }]);
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
      startWatcher: vi.fn(async () => {}),
      pollEvents: vi.fn(async () => queued.shift() ?? []),
    };
    const openAsync = vi.fn(async () => fakeInstance);
    const FakeDirSQL = Object.assign(
      vi.fn(function (this: unknown) {
        throw new Error("sync ctor should not be called");
      }),
      { openAsync },
    );

    (
      dirsql as unknown as {
        __setCoreForTesting: (c: { DirSQL: unknown }) => void;
      }
    ).__setCoreForTesting({ DirSQL: FakeDirSQL });

    const db = new dirsql.DirSQL({ root: "/tmp/nothing", tables: [] });
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

  // Regression test for https://github.com/thekevinscott/dirsql/issues/119
  // (the wrapper must `await` between polls) and
  // https://github.com/thekevinscott/dirsql/issues/147 (the native poll
  // itself runs on the libuv threadpool, so the JS event loop is never
  // parked for the poll duration). With both fixes, even a tight loop over
  // pollEvents returning [] yields to the event loop on every iteration,
  // so same-process setTimeout callbacks fire promptly.
  it("yields to the event loop between polls so same-process timers fire", async () => {
    let callCount = 0;
    // The mock returns a Promise that resolves on the next microtask. Bound
    // total calls so the test terminates cleanly even if the wrapper were
    // to regress to a sync loop.
    const pollEvents = vi.fn(async () => {
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
      startWatcher: vi.fn(async () => {}),
      pollEvents,
    };
    const openAsync = vi.fn(async () => fakeInstance);
    const FakeDirSQL = Object.assign(
      vi.fn(function (this: unknown) {
        throw new Error("sync ctor should not be called");
      }),
      { openAsync },
    );

    (
      dirsql as unknown as {
        __setCoreForTesting: (c: { DirSQL: unknown }) => void;
      }
    ).__setCoreForTesting({ DirSQL: FakeDirSQL });

    const db = new dirsql.DirSQL({ root: "/tmp/nothing", tables: [] });
    await db.ready;

    let timerFired = false;
    setTimeout(() => {
      timerFired = true;
    }, 5);

    const iter = db.watch()[Symbol.asyncIterator]();
    // Start the generator's poll loop. Don't await — pollEvents always
    // returns [] so the generator never hits `yield`; we force-terminate
    // via iter.return below.
    const pending = iter.next();
    pending.catch(() => {
      /* swallow — we force-terminate via iter.return below */
    });

    // Give the timer a generous window to fire. With the old sync-loop
    // bug, no setTimeout callback would fire before iter.return is called.
    await new Promise<void>((resolve) => setTimeout(resolve, 60));
    await iter.return?.();

    expect(timerFired).toBe(true);
    expect(pollEvents).toHaveBeenCalled();
  });
});
