// Hermetic: first-party code runs for real; the mocked boundaries are
// `node:fs` probes and `node:module`'s `createRequire` (behind which both
// the napi binary and package resolution sit). Real extension *loading* is
// covered by `tests/binding/`.

import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { DirSQL } from "dirsql";
import { beforeEach, describe, expect, it, vi } from "vitest";

// Fake native core module, delivered through the mocked `createRequire` so
// the real `loadNativeCore()` / `getCore()` chain runs unmodified.
const { fakeCore } = vi.hoisted(() => ({
  fakeCore: { DirSQL: { openAsync: vi.fn() } },
}));

vi.mock("node:fs", async (importOriginal) => ({
  ...(await importOriginal<typeof import("node:fs")>()),
  existsSync: vi.fn(),
  readFileSync: vi.fn(),
  readdirSync: vi.fn(),
  statSync: vi.fn(),
}));
vi.mock("node:module", async (importOriginal) => {
  const actual = await importOriginal<typeof import("node:module")>();
  // Called as `requirer(specifier)` by load-native-core: yield the fake core.
  const requirer = (_specifier: string) => fakeCore;
  // Read as `req.resolve` / `req.resolve.paths` by resolve-extension's
  // package prober: the fixture package resolves, everything else throws.
  requirer.resolve = (specifier: string): string => {
    if (specifier === "dirsql-testext-pkg/package.json") {
      return "/nm/dirsql-testext-pkg/package.json";
    }
    throw new Error(`Cannot find module '${specifier}'`);
  };
  requirer.resolve.paths = (): string[] => [];
  return {
    ...actual,
    createRequire: () => requirer as unknown as NodeJS.Require,
  };
});

const openAsync = fakeCore.DirSQL.openAsync;

beforeEach(() => {
  // Reset (not just clear) so per-test fs implementations never leak.
  vi.resetAllMocks();
  openAsync.mockResolvedValue({
    query: vi.fn().mockResolvedValue([]),
    startWatcher: vi.fn().mockResolvedValue(undefined),
    pollEvents: vi.fn().mockResolvedValue([]),
  });
});

describe("extensions option (hermetic, #230/#299)", () => {
  it("defaults to null when no extensions are configured", async () => {
    const db = new DirSQL({ root: "/data" });
    await db.ready;
    expect(openAsync.mock.calls[0]?.[6]).toBeNull();
  });

  it("forwards literal paths verbatim with their entrypoint", async () => {
    const db = new DirSQL({
      root: "/data",
      extensions: [
        { path: "/ext/libvec.so", entrypoint: "sqlite3_vec_init" },
        { path: "./rel/spellfix.so" },
      ],
    });
    await db.ready;
    expect(openAsync.mock.calls[0]?.[6]).toEqual([
      { path: "/ext/libvec.so", entrypoint: "sqlite3_vec_init" },
      { path: "./rel/spellfix.so", entrypoint: undefined },
    ]);
    expect(existsSync).not.toHaveBeenCalled();
  });

  it("resolves a bare package name to the installed loadable", async () => {
    // No same-named local file shadows the package...
    vi.mocked(existsSync).mockReturnValue(false);
    // ...and the resolved package dir contains exactly one platform loadable.
    vi.mocked(readdirSync).mockReturnValue([
      "libtestext.so",
    ] as unknown as ReturnType<typeof readdirSync>);

    const db = new DirSQL({
      root: "/data",
      extensions: [{ path: "dirsql-testext-pkg" }],
    });
    await db.ready;
    expect(openAsync.mock.calls[0]?.[6]).toEqual([
      { path: "/nm/dirsql-testext-pkg/libtestext.so", entrypoint: undefined },
    ]);
  });

  it("prefers a same-named local file over the installed package", async () => {
    const cwd = vi.spyOn(process, "cwd").mockReturnValue("/cwd");
    vi.mocked(existsSync).mockReturnValue(true);
    vi.mocked(statSync).mockReturnValue({
      isFile: () => true,
    } as unknown as ReturnType<typeof statSync>);

    const db = new DirSQL({
      root: "/data",
      extensions: [{ path: "dirsql-testext-pkg" }],
    });
    await db.ready;
    expect(openAsync.mock.calls[0]?.[6]).toEqual([
      { path: "/cwd/dirsql-testext-pkg", entrypoint: undefined },
    ]);
    cwd.mockRestore();
  });

  it("resolves config [[dirsql.extension]] package names and suppresses the core's own loading (#313)", async () => {
    // The config file exists; nothing else does (in particular no local file
    // shadows the package name).
    vi.mocked(existsSync).mockImplementation((p) => p === "/cfg/.dirsql.toml");
    vi.mocked(readFileSync).mockReturnValue(
      '[[dirsql.extension]]\npath = "dirsql-testext-pkg"\n',
    );
    vi.mocked(readdirSync).mockReturnValue([
      "libtestext.so",
    ] as unknown as ReturnType<typeof readdirSync>);

    const db = new DirSQL({ root: "/data", config: "/cfg/.dirsql.toml" });
    await db.ready;
    const call = openAsync.mock.calls[0];
    expect(call?.[6]).toEqual([
      { path: "/nm/dirsql-testext-pkg/libtestext.so", entrypoint: undefined },
    ]);
    expect(call?.[7]).toBe(true);
  });

  it("rejects ready when a bare package name is not installed", async () => {
    vi.mocked(existsSync).mockReturnValue(false);

    const db = new DirSQL({
      root: "/data",
      extensions: [{ path: "not-installed-pkg" }],
    });
    await expect(db.ready).rejects.toThrow(
      "could not resolve extension package 'not-installed-pkg': not installed",
    );
    expect(openAsync).not.toHaveBeenCalled();
  });
});
