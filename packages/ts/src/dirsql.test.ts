import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { getCore } from "./core.js";
import { DirSQL } from "./dirsql.js";
import { resolveConfigsExtensionSpecs } from "./resolve-config-extensions.js";
import { resolveExtensionPath } from "./resolve-extension.js";

vi.mock("./core.js");
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
  startWatcher: ReturnType<typeof vi.fn>;
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

function makeInner(overrides: Partial<FakeInner> = {}): FakeInner {
  return {
    query: vi.fn().mockResolvedValue([]),
    startWatcher: vi.fn().mockResolvedValue(undefined),
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
        "/data",
        [],
        ["*.tmp"],
        null,
        true,
        "/cache.db",
        null,
        false,
      );
      expect(resolveConfigsExtensionSpecs).not.toHaveBeenCalled();
    });

    it("forwards programmatic tables (including onFile) untouched as the second arg", async () => {
      const openAsync = installFakeCore(makeInner());
      const onFile = () => [];
      const tables = [
        { ddl: "CREATE TABLE t (n INTEGER)", glob: "**/*.json", onFile },
      ];
      const db = new DirSQL({ root: "/data", tables });
      await db.ready;
      expect(openAsync).toHaveBeenCalledWith(
        "/data",
        tables,
        null,
        null,
        null,
        null,
        null,
        false,
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
        false,
      );
    });

    it("treats a string argument as a config path", async () => {
      const openAsync = installFakeCore(makeInner());
      vi.mocked(resolveConfigsExtensionSpecs).mockReturnValue(null);
      const db = new DirSQL("/cfg.toml");
      await db.ready;
      expect(openAsync).toHaveBeenCalledWith(
        null,
        null,
        null,
        ["/cfg.toml"],
        null,
        null,
        null,
        false,
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
        null,
        null,
        null,
        ["/a.toml", "/b.toml"],
        null,
        null,
        null,
        false,
      );
    });

    it("forwards a null root when only a config is given (#540: the config never sets the root)", async () => {
      // The SDK never derives an index root from the config file's location;
      // it forwards `root: null` and the core defaults to the process cwd.
      const openAsync = installFakeCore(makeInner());
      vi.mocked(resolveConfigsExtensionSpecs).mockReturnValue(null);
      const db = new DirSQL({ config: "/elsewhere/.dirsql.toml" });
      await db.ready;
      expect(openAsync).toHaveBeenCalledWith(
        null,
        null,
        null,
        ["/elsewhere/.dirsql.toml"],
        null,
        null,
        null,
        false,
      );
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
        null,
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
        null,
        null,
        null,
        ["/cfg/.dirsql.toml"],
        null,
        null,
        [{ path: "/env/pkg/ext.so", entrypoint: undefined }],
        true,
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
        null,
        null,
        null,
        ["/cfg/.dirsql.toml"],
        null,
        null,
        [{ path: "R:ext/a.so", entrypoint: undefined }],
        false,
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

    it("close forwards to the inner instance", async () => {
      const db = new DirSQL({ root: "/d" });
      await db.ready;
      db.close();
      expect(inner.close).toHaveBeenCalledOnce();
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
