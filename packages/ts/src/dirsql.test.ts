import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { getCore } from "./core.js";
import { DirSQL } from "./dirsql.js";
import { resolveConfig } from "./resolve-config.js";

vi.mock("./core.js");
vi.mock("./resolve-config.js");

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

  it("toJSON delegates to resolveConfig with the stored options", () => {
    installFakeCore(makeInner());
    const resolved = {
      root: "/d",
      tables: [],
      ignore: [],
      persist: false,
      persistPath: null,
    };
    vi.mocked(resolveConfig).mockReturnValue(resolved);
    const db = new DirSQL({ root: "/d" });
    expect(db.toJSON()).toBe(resolved);
    expect(resolveConfig).toHaveBeenCalledWith({ root: "/d" });
  });

  describe("watch", () => {
    it("starts the watcher once and yields each polled event", async () => {
      const batches = [[], [{ table: "t", action: "insert" as const }]];
      const inner = makeInner({
        pollEvents: vi.fn(async () => batches.shift() ?? []),
      });
      installFakeCore(inner);

      const db = new DirSQL({ root: "/d" });
      const seen = [];
      for await (const ev of db.watch()) {
        seen.push(ev);
        break;
      }
      expect(inner.startWatcher).toHaveBeenCalledOnce();
      expect(seen).toEqual([{ table: "t", action: "insert" }]);
    });
  });
});
