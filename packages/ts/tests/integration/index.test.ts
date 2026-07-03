// Hermetic integration tests for the TypeScript SDK (#289).
//
// These exercise the SDK public API (the `dirsql` barrel) with the SDK's
// first-party code run for real and only the third-party boundary mocked:
// `node:module`'s `createRequire`, through which `load-native-core.ts`
// requires the napi binary, hands back a fake core module instead. They
// verify the SDK's glue — constructor overloads, positional marshaling into
// `openAsync`, ready/error propagation, method delegation, and the `watch()`
// async iterator — without loading the real napi binary or touching disk.
//
// Real-core behaviour (SQL semantics, scanning, diffing, watching) is
// covered by `tests/binding/` (the SDK against the real core) and by the
// Rust core's own suites.

import { DirSQL, type RowEvent, Table, parseTableName } from "dirsql";
import { beforeEach, describe, expect, it, vi } from "vitest";

// The fake native core module. `createRequire` below returns a requirer that
// resolves every specifier to this object, so the real `loadNativeCore()` /
// `getCore()` chain runs unmodified and lands here instead of `dirsql.node`.
const { fakeCore } = vi.hoisted(() => ({
  fakeCore: {
    DirSQL: { openAsync: vi.fn() },
    parseTableName: vi.fn(),
  },
}));

vi.mock("node:module", async (importOriginal) => {
  const actual = await importOriginal<typeof import("node:module")>();
  const requirer = (_specifier: string) => fakeCore;
  // `resolve-extension.ts`'s defaultResolver reads `req.resolve` /
  // `req.resolve.paths`; provide inert shapes so literal extension paths
  // (which never reach package resolution) keep working.
  requirer.resolve = (specifier: string): string => {
    throw new Error(`Cannot find module '${specifier}'`);
  };
  requirer.resolve.paths = (): string[] => [];
  return {
    ...actual,
    createRequire: () => requirer as unknown as NodeJS.Require,
  };
});

type FakeInner = {
  query: ReturnType<typeof vi.fn>;
  startWatcher: ReturnType<typeof vi.fn>;
  pollEvents: ReturnType<typeof vi.fn>;
};

function makeInner(overrides: Partial<FakeInner> = {}): FakeInner {
  return {
    query: vi.fn().mockResolvedValue([]),
    startWatcher: vi.fn().mockResolvedValue(undefined),
    pollEvents: vi.fn().mockResolvedValue([]),
    ...overrides,
  };
}

const openAsync = fakeCore.DirSQL.openAsync;

beforeEach(() => {
  vi.clearAllMocks();
});

describe("DirSQL construction", () => {
  it("treats a string argument as a config path", async () => {
    openAsync.mockResolvedValue(makeInner());
    const db = new DirSQL("/cfg/.dirsql.toml");
    await db.ready;
    expect(openAsync).toHaveBeenCalledWith(
      null,
      null,
      null,
      "/cfg/.dirsql.toml",
      null,
      null,
      null,
    );
  });

  it("maps an options object onto openAsync's positional args", async () => {
    openAsync.mockResolvedValue(makeInner());
    const tables = [
      {
        ddl: "CREATE TABLE t (n INTEGER)",
        glob: "**/*.json",
        extract: () => [],
      },
    ];
    const db = new DirSQL({
      root: "/data",
      tables,
      ignore: ["**/node_modules/**"],
      config: "/cfg/.dirsql.toml",
      persist: true,
      persistPath: "/cache.db",
    });
    await db.ready;
    expect(openAsync).toHaveBeenCalledWith(
      "/data",
      tables,
      ["**/node_modules/**"],
      "/cfg/.dirsql.toml",
      true,
      "/cache.db",
      null,
    );
  });

  it("accepts Table instances and plain objects interchangeably", async () => {
    openAsync.mockResolvedValue(makeInner());
    const extract = () => [];
    const asClass = new Table({
      ddl: "CREATE TABLE a (n INTEGER)",
      glob: "a/**",
      extract,
      strict: true,
    });
    const asLiteral = {
      ddl: "CREATE TABLE b (n INTEGER)",
      glob: "b/**",
      extract,
    };
    const db = new DirSQL({ root: "/data", tables: [asClass, asLiteral] });
    await db.ready;
    // The Table wrapper is structurally identical to the literal; both reach
    // the core unchanged.
    const [, forwarded] = openAsync.mock.calls[0] as [unknown, unknown[]];
    expect(forwarded).toEqual([
      {
        ddl: "CREATE TABLE a (n INTEGER)",
        glob: "a/**",
        extract,
        strict: true,
      },
      asLiteral,
    ]);
  });

  it("rejects ready when the core scan fails, and query surfaces the same error", async () => {
    openAsync.mockRejectedValue(new Error("no root directory"));
    const db = new DirSQL({});
    await expect(db.ready).rejects.toThrow("no root directory");
    await expect(db.query("SELECT 1")).rejects.toThrow("no root directory");
  });
});

describe("DirSQL delegation", () => {
  it("query awaits ready then forwards the SQL untouched", async () => {
    const inner = makeInner({
      query: vi.fn().mockResolvedValue([{ name: "ada" }]),
    });
    openAsync.mockResolvedValue(inner);
    const db = new DirSQL({ root: "/data" });
    // No explicit `await db.ready` — query must wait for the scan itself.
    const sql = "SELECT name FROM users WHERE age > 30 -- comment";
    expect(await db.query(sql)).toEqual([{ name: "ada" }]);
    expect(inner.query).toHaveBeenCalledWith(sql);
  });

  it("startWatcher and pollEvents delegate to the core instance", async () => {
    const events: RowEvent[] = [
      { table: "t", action: "insert", row: { n: 1 } },
    ];
    const inner = makeInner({
      pollEvents: vi.fn().mockResolvedValue(events),
    });
    openAsync.mockResolvedValue(inner);
    const db = new DirSQL({ root: "/data" });
    await db.startWatcher();
    expect(await db.pollEvents(50)).toEqual(events);
    expect(inner.startWatcher).toHaveBeenCalledOnce();
    expect(inner.pollEvents).toHaveBeenCalledWith(50);
  });
});

describe("DirSQL watch", () => {
  it("starts the watcher and yields events across poll batches", async () => {
    const batches: RowEvent[][] = [
      [
        { table: "t", action: "insert", row: { n: 1 } },
        { table: "t", action: "insert", row: { n: 2 } },
      ],
      [{ table: "t", action: "delete", row: { n: 1 } }],
    ];
    const inner = makeInner({
      pollEvents: vi.fn(async () => batches.shift() ?? []),
    });
    openAsync.mockResolvedValue(inner);

    const db = new DirSQL({ root: "/data" });
    const seen: RowEvent[] = [];
    for await (const event of db.watch()) {
      seen.push(event);
      if (seen.length >= 3) {
        break;
      }
    }
    expect(inner.startWatcher).toHaveBeenCalledOnce();
    expect(seen.map((e) => e.action)).toEqual(["insert", "insert", "delete"]);
  });
});

describe("parseTableName", () => {
  it("delegates DDL to the core's parser", () => {
    fakeCore.parseTableName.mockImplementation((ddl: string) =>
      ddl.includes("comments") ? "comments" : null,
    );
    expect(parseTableName('CREATE TABLE "comments" (n INTEGER)')).toBe(
      "comments",
    );
    expect(fakeCore.parseTableName).toHaveBeenCalledWith(
      'CREATE TABLE "comments" (n INTEGER)',
    );
  });
});
