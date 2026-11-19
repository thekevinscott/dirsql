import { resolve } from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { getCore } from "./core.js";
import { DirSQL, type RowEvent } from "./dirsql.js";
import { resolveConfigsExtensionSpecs } from "./resolve-config-extensions.js";
import { resolveExtensionPath } from "./resolve-extension.js";

vi.mock("./core.js");
vi.mock("node:path", async () => {
  const actual = await vi.importActual<typeof import("node:path")>("node:path");
  return {
    ...actual,
    default: actual,
    resolve: vi.fn((...parts: string[]) => `abs:${parts.join("|")}`),
  };
});
vi.mock("./resolve-extension.js", async () => ({
  ...(await vi.importActual<typeof import("./resolve-extension.js")>(
    "./resolve-extension.js",
  )),
  resolveExtensionPath: vi.fn((path: string) => `R:${path}`),
}));
vi.mock("./resolve-config-extensions.js", async () => ({
  ...(await vi.importActual<typeof import("./resolve-config-extensions.js")>(
    "./resolve-config-extensions.js",
  )),
  resolveConfigsExtensionSpecs: vi.fn(() => null),
}));

type FakeInner = {
  query: ReturnType<typeof vi.fn>;
  queryOrdered: ReturnType<typeof vi.fn>;
  startWatcher: ReturnType<typeof vi.fn>;
  stopWatcher: ReturnType<typeof vi.fn>;
  pollEvents: ReturnType<typeof vi.fn>;
  close: ReturnType<typeof vi.fn>;
};

function installFakeCore(inner: FakeInner) {
  const openAsync = vi.fn().mockResolvedValue(inner);
  vi.mocked(getCore).mockReturnValue({
    DirSQL: { openAsync },
  } as unknown as ReturnType<typeof getCore>);
  return openAsync;
}

// Resolves to the promise's value, or to "still pending" once a short wait
// passes, so a close that never wakes a waiting next() fails an assertion
// rather than hanging the test.
function settledSoon<T>(promise: Promise<T>): Promise<T | "still pending"> {
  return Promise.race([
    promise,
    new Promise<"still pending">((resolve) =>
      setTimeout(() => resolve("still pending"), 200),
    ),
  ]);
}

function makeInner(overrides: Partial<FakeInner> = {}): FakeInner {
  return {
    query: vi.fn().mockResolvedValue([]),
    queryOrdered: vi.fn().mockResolvedValue({ columns: [], rows: [] }),
    startWatcher: vi.fn(),
    stopWatcher: vi.fn(),
    pollEvents: vi.fn().mockResolvedValue([]),
    close: vi.fn(),
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
        resolve("/data"),
        [],
        ["*.tmp"],
        null,
        true,
        "/cache.db",
        null,
        false,
        null,
        null,
      );
      expect(resolveConfigsExtensionSpecs).not.toHaveBeenCalled();
    });

    it("forwards programmatic tables (including onFile) untouched as the second arg", async () => {
      const openAsync = installFakeCore(makeInner());
      const onFile = () => [];
      const tables = [
        {
          name: "t",
          ddl: "CREATE TABLE t (n INTEGER)",
          glob: "**/*.json",
          onFile,
        },
      ];
      const db = new DirSQL({ root: "/data", tables });
      await db.ready;
      expect(openAsync).toHaveBeenCalledWith(
        resolve("/data"),
        tables,
        null,
        null,
        null,
        null,
        null,
        false,
        null,
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
      expect(resolveExtensionPath).toHaveBeenCalledWith(
        "sqlite-vec",
        process.cwd(),
        true,
      );
      expect(openAsync).toHaveBeenCalledWith(
        resolve("/data"),
        null,
        null,
        null,
        null,
        null,
        [
          { path: "R:sqlite-vec", entrypoint: "sqlite3_vec_init" },
          { path: "R:/ext/spellfix.so", entrypoint: undefined },
        ],
        false,
        null,
        null,
      );
    });

    it("resolves extension paths against the cwd at construction", async () => {
      installFakeCore(makeInner());
      const cwd = vi.spyOn(process, "cwd").mockReturnValue("/built");
      const db = new DirSQL({
        root: "/data",
        extensions: [{ path: "ext/a.so" }],
      });
      cwd.mockRestore();
      await db.ready;
      expect(resolveExtensionPath).toHaveBeenCalledWith(
        "ext/a.so",
        "/built",
        true,
      );
    });

    it("reads an unreadable construction cwd again when resolving extensions", async () => {
      installFakeCore(makeInner());
      const cwd = vi.spyOn(process, "cwd").mockImplementationOnce(() => {
        throw new Error("ENOENT: uv_cwd");
      });
      const db = new DirSQL({
        root: "/data",
        extensions: [{ path: "ext/a.so" }],
      });
      cwd.mockReturnValue("/later");
      await db.ready;
      cwd.mockRestore();
      expect(resolveExtensionPath).toHaveBeenCalledWith(
        "ext/a.so",
        "/later",
        true,
      );
    });

    it("forwards noIgnore as openAsync's ninth arg (#746)", async () => {
      const openAsync = installFakeCore(makeInner());
      const db = new DirSQL({ root: "/data", noIgnore: true });
      await db.ready;
      expect(openAsync).toHaveBeenCalledWith(
        resolve("/data"),
        null,
        null,
        null,
        null,
        null,
        null,
        false,
        true,
        null,
      );
    });

    it("treats a string argument as a config path", async () => {
      const openAsync = installFakeCore(makeInner());
      vi.mocked(resolveConfigsExtensionSpecs).mockReturnValue(null);
      const db = new DirSQL("/cfg.toml");
      await db.ready;
      expect(openAsync).toHaveBeenCalledWith(
        resolve(process.cwd()),
        null,
        null,
        ["/cfg.toml"],
        null,
        null,
        null,
        false,
        null,
        null,
      );
      expect(db._options).toEqual({ config: "/cfg.toml" });
    });

    it("forwards an array of configs and resolves across them", async () => {
      const openAsync = installFakeCore(makeInner());
      vi.mocked(resolveConfigsExtensionSpecs).mockReturnValue(null);
      const db = new DirSQL({ config: ["/a.toml", "/b.toml"] });
      await db.ready;
      expect(resolveConfigsExtensionSpecs).toHaveBeenCalledWith([
        "/a.toml",
        "/b.toml",
      ]);
      expect(openAsync).toHaveBeenCalledWith(
        resolve(process.cwd()),
        null,
        null,
        ["/a.toml", "/b.toml"],
        null,
        null,
        null,
        false,
        null,
        null,
      );
    });

    it("roots at the cwd, not the config's directory, when only a config is given", async () => {
      const openAsync = installFakeCore(makeInner());
      vi.mocked(resolveConfigsExtensionSpecs).mockReturnValue(null);
      const db = new DirSQL({ config: "/elsewhere/.dirsql.toml" });
      await db.ready;
      expect(openAsync).toHaveBeenCalledWith(
        resolve(process.cwd()),
        null,
        null,
        ["/elsewhere/.dirsql.toml"],
        null,
        null,
        null,
        false,
        null,
        null,
      );
    });

    it("resolves the root before the constructor returns", () => {
      installFakeCore(makeInner());
      new DirSQL({ root: "tree" });
      expect(resolve).toHaveBeenCalledWith("tree");
    });

    it("leaves an unreadable cwd for the core to report", async () => {
      const openAsync = installFakeCore(makeInner());
      vi.mocked(resolve).mockImplementation(() => {
        throw new Error("ENOENT: uv_cwd");
      });
      const db = new DirSQL({ root: "tree" });
      await db.ready;
      expect(openAsync.mock.calls[0]?.[0]).toBe("tree");
    });

    it("appends resolved config extensions and suppresses the core's own loading", async () => {
      const openAsync = installFakeCore(makeInner());
      vi.mocked(resolveConfigsExtensionSpecs).mockReturnValue([
        { path: "/env/pkg/ext.so", entrypoint: "init" },
      ]);
      const db = new DirSQL({
        config: "/cfg/.dirsql.toml",
        extensions: [{ path: "ext/a.so" }],
      });
      await db.ready;
      expect(resolveConfigsExtensionSpecs).toHaveBeenCalledWith([
        "/cfg/.dirsql.toml",
      ]);
      expect(openAsync).toHaveBeenCalledWith(
        resolve(process.cwd()),
        null,
        null,
        ["/cfg/.dirsql.toml"],
        null,
        null,
        [
          { path: "R:ext/a.so", entrypoint: undefined },
          { path: "/env/pkg/ext.so", entrypoint: "init" },
        ],
        true,
        null,
        null,
      );
    });

    it("passes config extensions alone when there are no programmatic ones", async () => {
      const openAsync = installFakeCore(makeInner());
      vi.mocked(resolveConfigsExtensionSpecs).mockReturnValue([
        { path: "/env/pkg/ext.so", entrypoint: undefined },
      ]);
      const db = new DirSQL("/cfg/.dirsql.toml");
      await db.ready;
      expect(openAsync).toHaveBeenCalledWith(
        resolve(process.cwd()),
        null,
        null,
        ["/cfg/.dirsql.toml"],
        null,
        null,
        [{ path: "/env/pkg/ext.so", entrypoint: undefined }],
        true,
        null,
        null,
      );
    });

    it("leaves the core's loading untouched when the resolver declines", async () => {
      const openAsync = installFakeCore(makeInner());
      vi.mocked(resolveConfigsExtensionSpecs).mockReturnValue(null);
      const db = new DirSQL({
        config: "/cfg/.dirsql.toml",
        extensions: [{ path: "ext/a.so" }],
      });
      await db.ready;
      expect(openAsync).toHaveBeenCalledWith(
        resolve(process.cwd()),
        null,
        null,
        ["/cfg/.dirsql.toml"],
        null,
        null,
        [{ path: "R:ext/a.so", entrypoint: undefined }],
        false,
        null,
        null,
      );
    });
  });

  describe("construction failure", () => {
    it("surfaces the error at the query site with no unhandled rejection", async () => {
      const bootErr = new Error("boom: no such root");
      const openAsync = vi.fn().mockRejectedValue(bootErr);
      vi.mocked(getCore).mockReturnValue({
        DirSQL: { openAsync },
      } as unknown as ReturnType<typeof getCore>);

      const unhandled: unknown[] = [];
      const onUnhandled = (reason: unknown) => unhandled.push(reason);
      process.on("unhandledRejection", onUnhandled);

      const db = new DirSQL({ root: "./missing-root" });
      // Let the construction rejection settle before any handler is attached,
      // mirroring a caller that constructs at module load and queries later.
      await new Promise((resolve) => setTimeout(resolve, 0));

      await expect(db.query("SELECT 1")).rejects.toBe(bootErr);

      process.off("unhandledRejection", onUnhandled);
      expect(unhandled).toEqual([]);
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
      expect(inner.query).toHaveBeenCalledWith("SELECT 1", undefined);
    });

    it("query forwards its params to the inner instance", async () => {
      const db = new DirSQL({ root: "/d" });
      await db.query("SELECT ?, :a", [1]);
      await db.query("SELECT :a", { a: "x" });
      expect(inner.query).toHaveBeenNthCalledWith(1, "SELECT ?, :a", [1]);
      expect(inner.query).toHaveBeenNthCalledWith(2, "SELECT :a", { a: "x" });
    });

    it("queryOrdered awaits ready then forwards to the inner instance", async () => {
      const result = { columns: ["b", "a"], rows: [{ a: 1, b: 2 }] };
      inner.queryOrdered.mockResolvedValue(result);
      const db = new DirSQL({ root: "/d" });
      expect(await db.queryOrdered("SELECT b, a")).toEqual(result);
      expect(inner.queryOrdered).toHaveBeenCalledWith("SELECT b, a", undefined);
    });

    it("queryOrdered forwards its params to the inner instance", async () => {
      const db = new DirSQL({ root: "/d" });
      await db.queryOrdered("SELECT :a", { a: 1 });
      expect(inner.queryOrdered).toHaveBeenCalledWith("SELECT :a", { a: 1 });
    });

    it("startWatcher forwards to the inner instance once", async () => {
      const db = new DirSQL({ root: "/d" });
      await db.startWatcher();
      await db.startWatcher();
      expect(inner.startWatcher).toHaveBeenCalledOnce();
    });

    it("pollEvents forwards the timeout", async () => {
      const db = new DirSQL({ root: "/d" });
      await db.pollEvents(50);
      expect(inner.pollEvents).toHaveBeenCalledWith(50);
    });

    it("close forwards to the inner instance", async () => {
      const db = new DirSQL({ root: "/d" });
      await db.ready;
      db.close();
      expect(inner.close).toHaveBeenCalledOnce();
    });
  });

  describe("watch", () => {
    it("forwards watch: false as openAsync's tenth arg", async () => {
      const openAsync = installFakeCore(makeInner());
      const db = new DirSQL({ root: "/data", watch: false });
      await db.ready;
      expect(openAsync.mock.calls[0]?.[9]).toBe(false);
    });

    it("throws on watch() without starting the watcher when built unwatched", async () => {
      const inner = makeInner();
      installFakeCore(inner);
      const db = new DirSQL({ watch: false });
      expect(() => db.watch()).toThrow(/watching is off/);
      await db.ready;
      expect(() => db.watch()).toThrow(/watching is off/);
      expect(inner.startWatcher).not.toHaveBeenCalled();
    });

    it("starts the watcher before returning once ready", async () => {
      const inner = makeInner();
      installFakeCore(inner);
      const db = new DirSQL({});
      await db.ready;
      db.watch();
      expect(inner.startWatcher).toHaveBeenCalledOnce();
    });

    it("starts the watcher before ready resolves when called early", async () => {
      const inner = makeInner();
      installFakeCore(inner);
      const db = new DirSQL({});
      const stream = db.watch();
      expect(inner.startWatcher).not.toHaveBeenCalled();
      await db.ready;
      expect(inner.startWatcher).toHaveBeenCalledOnce();
      await stream.return();
    });

    it("leaves the watcher stopped without a stream", async () => {
      const inner = makeInner();
      installFakeCore(inner);
      const db = new DirSQL({});
      await db.ready;
      expect(inner.startWatcher).not.toHaveBeenCalled();
    });

    it("leaves an early start failure to the stream", async () => {
      const inner = makeInner({
        startWatcher: vi.fn(() => {
          throw new Error("no named tables");
        }),
      });
      installFakeCore(inner);
      const db = new DirSQL({});
      const stream = db.watch();
      await db.ready;
      await expect(stream.next()).rejects.toThrow("no named tables");
      expect(inner.startWatcher).toHaveBeenCalledTimes(2);
    });

    it("buffers each batch for every stream from creation", async () => {
      const events = [
        { table: "t", action: "insert" as const, row: { n: 1 } },
        { table: "t", action: "delete" as const, row: { n: 1 } },
      ];
      const inner = makeInner({
        pollEvents: vi
          .fn()
          .mockResolvedValueOnce(events)
          .mockRejectedValue(new Error("unexpected extra poll")),
      });
      installFakeCore(inner);
      const db = new DirSQL({});
      const first = db.watch();
      const second = db.watch();
      expect((await first.next()).value).toEqual(events[0]);
      expect((await second.next()).value).toEqual(events[0]);
      expect((await second.next()).value).toEqual(events[1]);
      expect((await first.next()).value).toEqual(events[1]);
      expect(inner.pollEvents).toHaveBeenCalledExactlyOnceWith(200);
      await first.return();
      await second.return();
    });

    it("accepts batches larger than the JavaScript argument limit", async () => {
      const event = { table: "t", action: "insert" as const };
      const inner = makeInner({
        pollEvents: vi.fn().mockResolvedValue(Array(150_000).fill(event)),
      });
      installFakeCore(inner);
      const stream = new DirSQL({}).watch();
      expect((await stream.next()).value).toEqual(event);
      await stream.return();
    });

    it("shares an in-flight poll between concurrent consumers", async () => {
      const event = { table: "t", action: "insert" as const };
      let release!: (events: (typeof event)[]) => void;
      const batch = new Promise<(typeof event)[]>((resolve) => {
        release = resolve;
      });
      const inner = makeInner({ pollEvents: vi.fn().mockReturnValue(batch) });
      installFakeCore(inner);
      const db = new DirSQL({});
      const first = db.watch();
      const second = db.watch();
      const pending = [first.next(), second.next()];
      await vi.waitFor(() => expect(inner.pollEvents).toHaveBeenCalled());
      release([event]);
      expect(await Promise.all(pending)).toEqual([
        { value: event, done: false },
        { value: event, done: false },
      ]);
      expect(inner.pollEvents).toHaveBeenCalledOnce();
      await first.return();
      await second.return();
    });

    it("does not replay an earlier batch to a later stream", async () => {
      const before = { table: "before", action: "insert" as const };
      const after = { table: "after", action: "delete" as const };
      const inner = makeInner({
        pollEvents: vi
          .fn()
          .mockResolvedValueOnce([before])
          .mockResolvedValueOnce([after]),
      });
      installFakeCore(inner);
      const db = new DirSQL({});
      const first = db.watch();
      expect((await first.next()).value).toEqual(before);
      const second = db.watch();
      expect((await second.next()).value).toEqual(after);
      expect((await first.next()).value).toEqual(after);
      expect(inner.pollEvents).toHaveBeenCalledTimes(2);
      await first.return();
      await second.return();
    });

    it("stops feeding a returned stream without losing another stream's batch", async () => {
      const event = { table: "t", action: "insert" as const };
      const inner = makeInner({
        pollEvents: vi.fn().mockResolvedValue([event]),
      });
      installFakeCore(inner);
      const db = new DirSQL({});
      const first = db.watch();
      const second = db.watch();
      await first.next();
      await first.return();
      const deref = vi.spyOn(WeakRef.prototype, "deref");
      try {
        expect((await second.next()).value).toEqual(event);
        expect((await second.next()).value).toEqual(event);
        expect(deref).toHaveBeenCalledTimes(2);
        await second.return();
      } finally {
        deref.mockRestore();
      }
    });

    it("removes collected streams from subsequent broadcasts", async () => {
      const event = { table: "t", action: "insert" as const };
      const inner = makeInner({
        pollEvents: vi.fn().mockResolvedValue([event]),
      });
      installFakeCore(inner);
      const db = new DirSQL({});
      await db.ready;
      db.watch();
      const kept = db.watch();
      const deref = vi
        .spyOn(WeakRef.prototype, "deref")
        .mockReturnValueOnce(undefined);
      try {
        expect((await kept.next()).value).toEqual(event);
        expect(deref).toHaveBeenCalledTimes(3);
        expect((await kept.next()).value).toEqual(event);
        expect(deref).toHaveBeenCalledTimes(5);
        await kept.return();
      } finally {
        deref.mockRestore();
      }
    });

    it("surfaces initialization and native poll errors", async () => {
      const inner = makeInner({
        pollEvents: vi.fn().mockRejectedValue(new Error("poll failed")),
      });
      const open = installFakeCore(inner);
      open.mockRejectedValueOnce(new Error("init failed"));
      await expect(new DirSQL({}).watch().next()).rejects.toThrow(
        "init failed",
      );
      await expect(new DirSQL({}).watch().next()).rejects.toThrow(
        "poll failed",
      );
    });

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

  describe("closing a stream", () => {
    it("ends iteration", async () => {
      const events = [
        { table: "t", action: "insert" as const },
        { table: "t", action: "delete" as const },
      ];
      const inner = makeInner({
        pollEvents: vi.fn().mockResolvedValue(events),
      });
      installFakeCore(inner);
      const stream = new DirSQL({}).watch();
      expect((await stream.next()).value).toEqual(events[0]);

      expect(await stream.return()).toEqual({ value: undefined, done: true });

      expect(await stream.next()).toEqual({ value: undefined, done: true });
      expect(inner.pollEvents).toHaveBeenCalledOnce();
    });

    it("stops the core when the last stream closes", async () => {
      const inner = makeInner({
        pollEvents: vi
          .fn()
          .mockResolvedValue([{ table: "t", action: "insert" }]),
      });
      installFakeCore(inner);
      const db = new DirSQL({});
      const first = db.watch();
      const second = db.watch();
      await first.next();

      await first.return();
      expect(inner.stopWatcher).not.toHaveBeenCalled();

      await second.return();
      expect(inner.stopWatcher).toHaveBeenCalledOnce();
    });

    it("stops feeding a closed stream", async () => {
      const a = { table: "a", action: "insert" as const };
      const b = { table: "b", action: "insert" as const };
      const inner = makeInner({
        pollEvents: vi.fn().mockResolvedValueOnce([a]).mockResolvedValue([b]),
      });
      installFakeCore(inner);
      const db = new DirSQL({});
      const closed = db.watch();
      const kept = db.watch();
      await closed.next();
      await closed.return();
      await kept.next();

      expect((await kept.next()).value).toEqual(b);
      expect(await closed.next()).toEqual({ value: undefined, done: true });
    });

    it("can be closed twice", async () => {
      const inner = makeInner();
      installFakeCore(inner);
      const db = new DirSQL({});
      await db.ready;
      const stream = db.watch();

      await stream.return();
      await stream.return();

      expect(await stream.next()).toEqual({ value: undefined, done: true });
      expect(inner.stopWatcher).toHaveBeenCalledOnce();
    });

    it("never starts the watcher for a stream closed before the scan ends", async () => {
      const inner = makeInner();
      installFakeCore(inner);
      const db = new DirSQL({});
      const stream = db.watch();

      await stream.return();
      await db.ready;

      expect(await stream.next()).toEqual({ value: undefined, done: true });
      expect(inner.startWatcher).not.toHaveBeenCalled();
      expect(inner.pollEvents).not.toHaveBeenCalled();
    });

    it("never starts the watcher when closed while its first next() awaits the scan", async () => {
      const inner = makeInner({
        pollEvents: vi.fn(() => new Promise(() => {})),
      });
      installFakeCore(inner);
      const db = new DirSQL({});
      const stream = db.watch();
      const first = stream.next();

      const closing = stream.return();

      expect(await settledSoon(first)).toEqual({
        value: undefined,
        done: true,
      });
      await closing;
      expect(inner.startWatcher).not.toHaveBeenCalled();
      expect(inner.pollEvents).not.toHaveBeenCalled();
    });

    it("wakes a next() waiting on a poll, and leaves the stop to that poll", async () => {
      let release!: (events: RowEvent[]) => void;
      const inner = makeInner({
        pollEvents: vi.fn(
          () =>
            new Promise<RowEvent[]>((resolve) => {
              release = resolve;
            }),
        ),
      });
      installFakeCore(inner);
      const stream = new DirSQL({}).watch();
      const waiting = stream.next();
      await vi.waitFor(() => expect(inner.pollEvents).toHaveBeenCalled());

      const closing = stream.return();

      expect(await settledSoon(waiting)).toEqual({
        value: undefined,
        done: true,
      });
      expect(inner.stopWatcher).not.toHaveBeenCalled();
      release([{ table: "t", action: "insert" }]);
      await closing;
      await vi.waitFor(() => expect(inner.stopWatcher).toHaveBeenCalledOnce());
    });

    it("closes the stream when a poll fails", async () => {
      const inner = makeInner({
        pollEvents: vi.fn().mockRejectedValue(new Error("poll failed")),
      });
      installFakeCore(inner);
      const stream = new DirSQL({}).watch();

      await expect(stream.next()).rejects.toThrow("poll failed");

      expect(await stream.next()).toEqual({ value: undefined, done: true });
      expect(inner.stopWatcher).toHaveBeenCalledOnce();
    });

    it("closes the stream on throw() and rejects with the error", async () => {
      const inner = makeInner();
      installFakeCore(inner);
      const db = new DirSQL({});
      await db.ready;
      const stream = db.watch();

      await expect(stream.throw(new Error("consumer failed"))).rejects.toThrow(
        "consumer failed",
      );

      expect(await stream.next()).toEqual({ value: undefined, done: true });
      expect(inner.stopWatcher).toHaveBeenCalledOnce();
    });

    it("is its own async iterator", () => {
      installFakeCore(makeInner());
      const stream = new DirSQL({}).watch();
      expect(stream[Symbol.asyncIterator]()).toBe(stream);
    });

    it("stops the core when the loop is left", async () => {
      const inner = makeInner({
        pollEvents: vi
          .fn()
          .mockResolvedValue([{ table: "t", action: "insert" }]),
      });
      installFakeCore(inner);
      for await (const _ of new DirSQL({}).watch()) {
        break;
      }
      expect(inner.stopWatcher).toHaveBeenCalledOnce();
    });

    it("starts the watcher again after the last stream closed", async () => {
      const inner = makeInner();
      installFakeCore(inner);
      const db = new DirSQL({});
      await db.ready;
      await db.watch().return();

      db.watch();

      expect(inner.startWatcher).toHaveBeenCalledTimes(2);
    });

    it("has nothing to stop when the stream closes before the core is built", async () => {
      const inner = makeInner();
      installFakeCore(inner);
      const db = new DirSQL({});

      await db.watch().return();
      await db.ready;

      expect(inner.stopWatcher).not.toHaveBeenCalled();
      expect(inner.startWatcher).not.toHaveBeenCalled();
    });

    it("keeps the core attached for a caller of startWatcher()", async () => {
      const inner = makeInner();
      installFakeCore(inner);
      const db = new DirSQL({});
      await db.startWatcher();

      await db.watch().return();

      expect(inner.stopWatcher).not.toHaveBeenCalled();
    });

    it("keeps the core attached for a caller of pollEvents()", async () => {
      const inner = makeInner();
      installFakeCore(inner);
      const db = new DirSQL({});
      await db.pollEvents(0);

      await db.watch().return();

      expect(inner.stopWatcher).not.toHaveBeenCalled();
    });
  });

  describe("dropped stream", () => {
    type Registry = {
      cleanups: ((held: unknown) => void)[];
      registered: object[];
      unregistered: object[];
    };

    function stubFinalizationRegistry(): Registry {
      const registry: Registry = {
        cleanups: [],
        registered: [],
        unregistered: [],
      };
      vi.stubGlobal(
        "FinalizationRegistry",
        class {
          constructor(cleanup: (held: unknown) => void) {
            registry.cleanups.push(cleanup);
          }
          register(_target: object, _held: unknown, token: object) {
            registry.registered.push(token);
          }
          unregister(token: object) {
            registry.unregistered.push(token);
            return true;
          }
        },
      );
      return registry;
    }

    afterEach(() => vi.unstubAllGlobals());

    it("stops the core when the last stream is collected", async () => {
      const registry = stubFinalizationRegistry();
      const inner = makeInner();
      installFakeCore(inner);
      const db = new DirSQL({});
      await db.ready;
      db.watch();
      expect(registry.cleanups).toHaveLength(1);
      expect(registry.registered).toHaveLength(1);
      const deref = vi
        .spyOn(WeakRef.prototype, "deref")
        .mockReturnValue(undefined);
      try {
        registry.cleanups[0]?.(null);
      } finally {
        deref.mockRestore();
      }

      expect(inner.stopWatcher).toHaveBeenCalledOnce();
    });

    it("keeps the core attached while another stream lives", async () => {
      const registry = stubFinalizationRegistry();
      const inner = makeInner();
      installFakeCore(inner);
      const db = new DirSQL({});
      await db.ready;
      db.watch();
      const kept = db.watch();
      const deref = vi
        .spyOn(WeakRef.prototype, "deref")
        .mockReturnValueOnce(undefined);
      try {
        expect(registry.cleanups).toHaveLength(1);
        registry.cleanups[0]?.(null);
      } finally {
        deref.mockRestore();
      }

      expect(inner.stopWatcher).not.toHaveBeenCalled();
      await kept.return();
    });

    it("has nothing to collect once the stream is closed", async () => {
      const registry = stubFinalizationRegistry();
      installFakeCore(makeInner());
      const stream = new DirSQL({}).watch();

      await stream.return();

      expect(registry.registered).toHaveLength(1);
      expect(registry.unregistered).toEqual(registry.registered);
    });
  });
});
