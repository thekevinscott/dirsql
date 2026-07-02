import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { getCore } from "./core.js";
import { DirSQL } from "./dirsql.js";
import { resolveExtensionPath } from "./resolve-extension.js";

vi.mock("./core.js");
// Extension path resolution (file-vs-package) is unit-tested in
// `resolve-extension.test`; here it is mocked so construction asserts only that
// the resolved specs reach `openAsync`.
vi.mock("./resolve-extension.js", async () => ({
  ...(await vi.importActual<typeof import("./resolve-extension.js")>(
    "./resolve-extension.js",
  )),
  resolveExtensionPath: vi.fn((path: string) => `R:${path}`),
}));

type FakeInner = {
  query: ReturnType<typeof vi.fn>;
  startWatcher: ReturnType<typeof vi.fn>;
  pollEvents: ReturnType<typeof vi.fn>;
};

function installFakeCore(inner: FakeInner) {
  const openAsync = vi.fn().mockResolvedValue(inner);
  vi.mocked(getCore).mockReturnValue({
    DirSQL: { openAsync },
  } as unknown as ReturnType<typeof getCore>);
  return openAsync;
}

function makeInner(overrides: Partial<FakeInner> = {}): FakeInner {
  return {
    query: vi.fn().mockResolvedValue([]),
    startWatcher: vi.fn().mockResolvedValue(undefined),
    pollEvents: vi.fn().mockResolvedValue([]),
    ...overrides,
  };
}

describe("DirSQL", () => {
  afterEach(() => vi.resetAllMocks());

  describe("construction", () => {
    it("maps an options object onto openAsync's positional args", async () => {
      const openAsync = installFakeCore(makeInner());
      const db = new DirSQL({
        root: "/data",
        tables: [],
        ignore: ["*.tmp"],
        persist: true,
        persistPath: "/cache.db",
      });
      await db.ready;
      expect(openAsync).toHaveBeenCalledWith(
        "/data",
        [],
        ["*.tmp"],
        null,
        true,
        "/cache.db",
        null,
      );
    });

    it("resolves extension paths before forwarding them as openAsync's seventh arg", async () => {
      const openAsync = installFakeCore(makeInner());
      const db = new DirSQL({
        root: "/data",
        extensions: [
          { path: "sqlite-vec", entrypoint: "sqlite3_vec_init" },
          { path: "/ext/spellfix.so" },
        ],
      });
      await db.ready;
      // Each path is routed through the (mocked) resolver before reaching the
      // core; a bare name resolves against cwd without being made absolute.
      expect(resolveExtensionPath).toHaveBeenCalledWith(
        "sqlite-vec",
        process.cwd(),
        false,
      );
      expect(openAsync).toHaveBeenCalledWith(
        "/data",
        null,
        null,
        null,
        null,
        null,
        [
          { path: "R:sqlite-vec", entrypoint: "sqlite3_vec_init" },
          { path: "R:/ext/spellfix.so", entrypoint: undefined },
        ],
      );
    });

    it("treats a string argument as a config path", async () => {
      const openAsync = installFakeCore(makeInner());
      const db = new DirSQL("/cfg.toml");
      await db.ready;
      expect(openAsync).toHaveBeenCalledWith(
        null,
        null,
        null,
        "/cfg.toml",
        null,
        null,
        null,
      );
      expect(db._options).toEqual({ config: "/cfg.toml" });
    });
  });

  describe("delegation", () => {
    let inner: FakeInner;

    beforeEach(() => {
      inner = makeInner({ query: vi.fn().mockResolvedValue([{ ok: 1 }]) });
      installFakeCore(inner);
    });

    it("query awaits ready then forwards to the inner instance", async () => {
      const db = new DirSQL({ root: "/d" });
      expect(await db.query("SELECT 1")).toEqual([{ ok: 1 }]);
      expect(inner.query).toHaveBeenCalledWith("SELECT 1");
    });

    it("startWatcher forwards to the inner instance", async () => {
      const db = new DirSQL({ root: "/d" });
      await db.startWatcher();
      expect(inner.startWatcher).toHaveBeenCalledOnce();
    });

    it("pollEvents forwards the timeout", async () => {
      const db = new DirSQL({ root: "/d" });
      await db.pollEvents(50);
      expect(inner.pollEvents).toHaveBeenCalledWith(50);
    });
  });

  describe("watch", () => {
    it("starts the watcher once and yields each polled event across polls", async () => {
      // Two events split across two non-empty polls (with an empty poll
      // first) so the test exercises the generator continuing its loop after
      // a yield, not just the first event.
      const batches = [
        [],
        [{ table: "t", action: "insert" as const, row: { n: 1 } }],
        [{ table: "t", action: "insert" as const, row: { n: 2 } }],
      ];
      const inner = makeInner({
        pollEvents: vi.fn(async () => batches.shift() ?? []),
      });
      installFakeCore(inner);

      const db = new DirSQL({ root: "/d" });
      const seen = [];
      for await (const ev of db.watch()) {
        seen.push(ev);
        if (seen.length >= 2) {
          break;
        }
      }
      expect(inner.startWatcher).toHaveBeenCalledOnce();
      expect(seen).toEqual([
        { table: "t", action: "insert", row: { n: 1 } },
        { table: "t", action: "insert", row: { n: 2 } },
      ]);
    });

    // Regression for https://github.com/thekevinscott/dirsql/issues/119 (the
    // wrapper must `await` between polls) and #147 (the native poll runs on
    // the libuv threadpool). Even a tight loop over pollEvents returning []
    // must yield to the event loop each iteration, so a same-process
    // setTimeout still fires.
    it("yields to the event loop between polls so same-process timers fire", async () => {
      let calls = 0;
      const inner = makeInner({
        pollEvents: vi.fn(async () => {
          calls += 1;
          if (calls > 200) {
            throw new Error(
              "watch() did not yield to the event loop between polls",
            );
          }
          return [];
        }),
      });
      installFakeCore(inner);

      const db = new DirSQL({ root: "/d" });
      await db.ready;

      let timerFired = false;
      setTimeout(() => {
        timerFired = true;
      }, 5);

      const iter = db.watch()[Symbol.asyncIterator]();
      // pollEvents always returns [], so the generator never hits `yield`;
      // start it without awaiting and force-terminate via iter.return below.
      const pending = iter.next();
      pending.catch(() => {
        /* swallow -- force-terminated below */
      });

      await new Promise<void>((resolve) => setTimeout(resolve, 60));
      await iter.return?.();

      expect(timerFired).toBe(true);
      expect(inner.pollEvents).toHaveBeenCalled();
    });
  });
});
