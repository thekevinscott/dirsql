import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ExtensionPlanEntry } from "./core.js";
import { getCore } from "./core.js";
import { planEntryPath } from "./plan-entry-path.js";
import { readConfig } from "./read-config.js";
import { resolveConfigsExtensionSpecs } from "./resolve-config-extensions.js";

vi.mock("./core.js");
vi.mock("./plan-entry-path.js", async () => ({
  ...(await vi.importActual<typeof import("./plan-entry-path.js")>(
    "./plan-entry-path.js",
  )),
  planEntryPath: vi.fn(),
}));
vi.mock("./read-config.js", async () => ({
  ...(await vi.importActual<typeof import("./read-config.js")>(
    "./read-config.js",
  )),
  readConfig: vi.fn(),
}));

const planConfigExtensions =
  vi.fn<(configs: unknown[]) => ExtensionPlanEntry[] | null>();

function packageEntry(name: string, entrypoint?: string): ExtensionPlanEntry {
  return { package: name, shadow: `/cfg/${name}`, entrypoint };
}

beforeEach(() => {
  vi.mocked(getCore).mockReturnValue({ planConfigExtensions } as never);
});

afterEach(() => vi.resetAllMocks());

describe("resolveConfigsExtensionSpecs", () => {
  it("hands the core each config's absolute path and contents", () => {
    vi.mocked(readConfig).mockReturnValue('[[dirsql.extension]]\npath = "x"\n');
    planConfigExtensions.mockReturnValue(null);

    resolveConfigsExtensionSpecs(["/a/.dirsql.toml"]);

    expect(planConfigExtensions).toHaveBeenCalledWith([
      {
        path: "/a/.dirsql.toml",
        contents: '[[dirsql.extension]]\npath = "x"\n',
      },
    ]);
  });

  it("leaves an unreadable config's contents undefined", () => {
    vi.mocked(readConfig).mockReturnValue(undefined);
    planConfigExtensions.mockReturnValue(null);

    resolveConfigsExtensionSpecs(["/gone/.dirsql.toml"]);

    expect(planConfigExtensions).toHaveBeenCalledWith([
      { path: "/gone/.dirsql.toml", contents: undefined },
    ]);
  });

  it("returns null when the core does not plan anything", () => {
    planConfigExtensions.mockReturnValue(null);
    expect(resolveConfigsExtensionSpecs(["/a/.dirsql.toml"])).toBeNull();
    expect(planEntryPath).not.toHaveBeenCalled();
  });

  it("resolves every planned entry, in order", () => {
    planConfigExtensions.mockReturnValue([
      packageEntry("sqlite_vec", "sqlite3_vec_init"),
      packageEntry("spellfix"),
    ]);
    vi.mocked(planEntryPath).mockImplementation(
      (entry) => `/nm/${entry.package}.so`,
    );

    expect(
      resolveConfigsExtensionSpecs(["/a/.dirsql.toml", "/b/.dirsql.toml"]),
    ).toEqual([
      { path: "/nm/sqlite_vec.so", entrypoint: "sqlite3_vec_init" },
      { path: "/nm/spellfix.so", entrypoint: undefined },
    ]);
  });
});
