// Hermetic red tests for #570: the per-file row seam is `onFile`, not
// `extract`. One name for the seam on every surface: `on-file` in TOML,
// `onFile` in the TypeScript SDK. The old `extract` property is a hard
// break (no deprecation alias).
//
// Same mocked boundary as `index.test.ts`: the only fake is `node:module`'s
// `createRequire`, so `loadNativeCore()` lands on a fake core module.

import { DirSQL, Table } from "dirsql";
import { beforeEach, describe, expect, it, vi } from "vitest";

const { fakeCore } = vi.hoisted(() => ({
  fakeCore: {
    DirSQL: { openAsync: vi.fn() },
  },
}));

vi.mock("node:module", async (importOriginal) => {
  const actual = await importOriginal<typeof import("node:module")>();
  const requirer = (_specifier: string) => fakeCore;
  requirer.resolve = (specifier: string): string => {
    throw new Error(`Cannot find module '${specifier}'`);
  };
  requirer.resolve.paths = (): string[] => [];
  return {
    ...actual,
    createRequire: () => requirer as unknown as NodeJS.Require,
  };
});

const openAsync = fakeCore.DirSQL.openAsync;

function makeInner() {
  return {
    query: vi.fn().mockResolvedValue([]),
    startWatcher: vi.fn().mockResolvedValue(undefined),
    pollEvents: vi.fn().mockResolvedValue([]),
  };
}

beforeEach(() => {
  vi.clearAllMocks();
});

describe("the onFile table seam", () => {
  it("forwards a table's onFile callback to the core", async () => {
    openAsync.mockResolvedValue(makeInner());
    const onFile = () => [];
    const db = new DirSQL({
      root: "/data",
      tables: [
        {
          name: "t",
          ddl: "CREATE TABLE t (n INTEGER)",
          glob: "**/*.json",
          onFile,
        },
      ],
    });
    await db.ready;
    const [, forwarded] = openAsync.mock.calls[0] as [
      unknown,
      Array<Record<string, unknown>>,
    ];
    expect(forwarded).toEqual([
      {
        name: "t",
        ddl: "CREATE TABLE t (n INTEGER)",
        glob: "**/*.json",
        onFile,
      },
    ]);
  });

  it("Table copies onFile and carries no extract property", () => {
    const onFile = () => [];
    const table = new Table({
      name: "t",
      ddl: "CREATE TABLE t (n INTEGER)",
      glob: "**/*.json",
      onFile,
    });
    expect(table.onFile).toBe(onFile);
    expect(table).not.toHaveProperty("extract");
  });
});
