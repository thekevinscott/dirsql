import { existsSync, statSync } from "node:fs";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ExtensionPlanEntry } from "./core.js";
import { defaultResolver } from "./default-resolver.js";
import type { PackageResolver } from "./package-dir.js";
import { planEntryPath } from "./plan-entry-path.js";
import { resolvePackage } from "./resolve-package.js";

vi.mock("node:fs", async () => ({
  ...(await vi.importActual<typeof import("node:fs")>("node:fs")),
  existsSync: vi.fn(),
  statSync: vi.fn(),
}));
vi.mock("./default-resolver.js", async () => ({
  ...(await vi.importActual<typeof import("./default-resolver.js")>(
    "./default-resolver.js",
  )),
  defaultResolver: vi.fn(),
}));
vi.mock("./resolve-package.js", async () => ({
  ...(await vi.importActual<typeof import("./resolve-package.js")>(
    "./resolve-package.js",
  )),
  resolvePackage: vi.fn(),
}));

const resolver = { resolve: vi.fn(), paths: vi.fn() } as PackageResolver;

function packageEntry(): ExtensionPlanEntry {
  return { package: "sqlite_vec", shadow: "/cfg/sqlite_vec" };
}

function stat(isFile: boolean) {
  vi.mocked(statSync).mockReturnValue({
    isFile: () => isFile,
  } as unknown as ReturnType<typeof statSync>);
}

afterEach(() => vi.resetAllMocks());

describe("planEntryPath", () => {
  it("returns a literal entry's path without probing the filesystem", () => {
    const entry: ExtensionPlanEntry = { path: "/cfg/vec0.so" };
    expect(planEntryPath(entry, resolver)).toBe("/cfg/vec0.so");
    expect(existsSync).not.toHaveBeenCalled();
    expect(resolvePackage).not.toHaveBeenCalled();
  });

  it("prefers a same-named file shadowing the package", () => {
    vi.mocked(existsSync).mockReturnValue(true);
    stat(true);
    expect(planEntryPath(packageEntry(), resolver)).toBe("/cfg/sqlite_vec");
    expect(resolvePackage).not.toHaveBeenCalled();
  });

  it("ignores a shadow that is a directory", () => {
    vi.mocked(existsSync).mockReturnValue(true);
    stat(false);
    vi.mocked(resolvePackage).mockReturnValue("/nm/sqlite_vec/vec0.so");
    expect(planEntryPath(packageEntry(), resolver)).toBe(
      "/nm/sqlite_vec/vec0.so",
    );
  });

  it("falls back to the installed package", () => {
    vi.mocked(existsSync).mockReturnValue(false);
    vi.mocked(resolvePackage).mockReturnValue("/nm/sqlite_vec/vec0.so");
    expect(planEntryPath(packageEntry(), resolver)).toBe(
      "/nm/sqlite_vec/vec0.so",
    );
    expect(resolvePackage).toHaveBeenCalledWith("sqlite_vec", resolver);
  });

  it("defaults to the require.resolve-backed resolver", () => {
    const fallback = { resolve: vi.fn(), paths: vi.fn() } as PackageResolver;
    vi.mocked(defaultResolver).mockReturnValue(fallback);
    vi.mocked(existsSync).mockReturnValue(false);
    vi.mocked(resolvePackage).mockReturnValue("/nm/sqlite_vec/vec0.so");

    expect(planEntryPath(packageEntry())).toBe("/nm/sqlite_vec/vec0.so");
    expect(resolvePackage).toHaveBeenCalledWith("sqlite_vec", fallback);
  });
});
