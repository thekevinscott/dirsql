// Hermetic: first-party code runs for real; the only mocked boundary is
// `node:module`'s `createRequire`, through which `load-native-core.ts`
// requires the napi binary — it hands back a fake core module instead.
// Real-core behaviour is covered by `tests/binding/`.

import { DirSQL, type RowEvent, Table, parseTableName } from "dirsql";
import { beforeEach, describe, expect, it, vi } from "vitest";

// The mocked `createRequire` resolves every specifier to this object, so the
// real `loadNativeCore()` / `getCore()` chain runs unmodified and lands here
// instead of `dirsql.node`.
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
      ["/cfg/.dirsql.toml"],
      null,
      null,
      null,
      false,
    );
  });

  it("maps an options object onto openAsync's positional args", async () => {
    openAsync.mockResolvedValue(makeInner());
    const tables = [
      {
        ddl: "CREATE TABLE t (n INTEGER)",
        glob: "**/*.json",
        onFile: () => [],
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
      ["/cfg/.dirsql.toml"],
      true,
      "/cache.db",
      null,
      false,
    );
  });

  it("accepts Table instances and plain objects interchangeably", async () => {
    openAsync.mockResolvedValue(makeInner());
    const onFile = () => [];
    const asClass = new Table({
      ddl: "CREATE TABLE a (n INTEGER)",
      glob: "a/**",
      onFile,
      strict: true,
    });
    const asLiteral = {
      ddl: "CREATE TABLE b (n INTEGER)",
      glob: "b/**",
      onFile,
    };
    const db = new DirSQL({ root: "/data", tables: [asClass, asLiteral] });
    await db.ready;
    const [, forwarded] = openAsync.mock.calls[0] as [unknown, unknown[]];
    expect(forwarded).toEqual([
      {
        ddl: "CREATE TABLE a (n INTEGER)",
        glob: "a/**",
        onFile,
        strict: true,
      },
      asLiteral,
    ]);
  });

  it("rejects ready when the core scan fails, and query surfaces the same error", async () => {
    openAsync.mockRejectedValue(new Error("core scan failed"));
    const db = new DirSQL({});
    await expect(db.ready).rejects.toThrow("core scan failed");
    await expect(db.query("SELECT 1")).rejects.toThrow("core scan failed");
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

  it("forwards a path-table name without rewriting it", async () => {
    // Path-table resolution lives in the core; the SDK must not rewrite,
    // quote, or normalize the name on the way down.
    const inner = makeInner({ query: vi.fn().mockResolvedValue([]) });
    openAsync.mockResolvedValue(inner);
    const db = new DirSQL({ root: "/data" });
    const sql = "SELECT basename FROM './docs/*.md'";
    await db.query(sql);
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
