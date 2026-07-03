// Hermetic integration tests for the `extensions` constructor option (#289).
//
// These exercise the SDK public API with first-party code run for real and
// only the third-party boundaries mocked: `node:fs` probes and
// `node:module`'s `createRequire` (which both `load-native-core.ts` — the
// napi binary — and `resolve-extension.ts` — package resolution — sit
// behind). Programmatic extension entries flow through the real
// `src/resolve-extension.ts` and must reach `openAsync`'s seventh argument
// resolved. Real extension *loading* (a missing `.so` failing ready, a
// fixture cdylib registering a function) is covered by `tests/binding/`.

import { existsSync, readdirSync, statSync } from "node:fs";
import { DirSQL } from "dirsql";
import { beforeEach, describe, expect, it, vi } from "vitest";

// Fake native core module, delivered through the mocked `createRequire` so
// the real `loadNativeCore()` / `getCore()` chain runs unmodified.
const { fakeCore } = vi.hoisted(() => ({
  fakeCore: { DirSQL: { openAsync: vi.fn() }, parseTableName: vi.fn() },
}));

vi.mock("node:fs", async (importOriginal) => ({
  ...(await importOriginal<typeof import("node:fs")>()),
  existsSync: vi.fn(),
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
  vi.clearAllMocks();
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
    // Programmatic path-looking entries pass through verbatim (the core
    // resolves them); no fs probe runs for either.
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

  it("rejects ready when a bare package name is not installed", async () => {
    vi.mocked(existsSync).mockReturnValue(false);

    const db = new DirSQL({
      root: "/data",
      extensions: [{ path: "not-installed-pkg" }],
    });
    await expect(db.ready).rejects.toThrow(
      "could not resolve extension package 'not-installed-pkg': not installed",
    );
    // The resolution error surfaced before the core was ever opened.
    expect(openAsync).not.toHaveBeenCalled();
  });
});
