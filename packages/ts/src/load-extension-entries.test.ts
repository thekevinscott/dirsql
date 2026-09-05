import { existsSync, readFileSync } from "node:fs";
import { parse as parseToml } from "smol-toml";
import { afterEach, describe, expect, it, vi } from "vitest";
import { loadExtensionEntries } from "./load-extension-entries.js";

vi.mock("node:fs", async () => ({
  ...(await vi.importActual<typeof import("node:fs")>("node:fs")),
  existsSync: vi.fn(),
  readFileSync: vi.fn(() => ""),
}));
vi.mock("smol-toml", async () => ({
  ...(await vi.importActual<typeof import("smol-toml")>("smol-toml")),
  parse: vi.fn(),
}));

function stubConfig(doc: unknown, exists = true) {
  vi.mocked(existsSync).mockReturnValue(exists);
  vi.mocked(parseToml).mockReturnValue(doc as ReturnType<typeof parseToml>);
}

describe("loadExtensionEntries", () => {
  afterEach(() => vi.resetAllMocks());

  it("returns null when the config file does not exist", () => {
    vi.mocked(existsSync).mockReturnValue(false);
    expect(loadExtensionEntries("/nope/.dirsql.toml")).toBeNull();
    expect(existsSync).toHaveBeenCalledWith("/nope/.dirsql.toml");
    expect(readFileSync).not.toHaveBeenCalled();
  });

  it("returns null when the TOML fails to parse", () => {
    vi.mocked(existsSync).mockReturnValue(true);
    vi.mocked(parseToml).mockImplementation(() => {
      throw new Error("bad toml");
    });
    expect(loadExtensionEntries("/x/.dirsql.toml")).toBeNull();
  });

  it("returns null when the config has no [dirsql] section at all", () => {
    stubConfig({
      table: [{ name: "t", ddl: "CREATE TABLE t (x TEXT)", glob: "*" }],
    });
    expect(loadExtensionEntries("/x/.dirsql.toml")).toBeNull();
  });

  it("returns null when the config declares no extensions", () => {
    stubConfig({ dirsql: { ignore: ["x"] } });
    expect(loadExtensionEntries("/x/.dirsql.toml")).toBeNull();
  });

  it("returns null when `extension` is not an array", () => {
    stubConfig({ dirsql: { extension: "not-a-list" } });
    expect(loadExtensionEntries("/x/.dirsql.toml")).toBeNull();
  });

  it("returns the entries with the config's parent directory", () => {
    const entries = [{ path: "sqlite_vec" }, { path: "ext/a.so" }];
    stubConfig({ dirsql: { extension: entries } });
    expect(loadExtensionEntries("/cfg/nested/.dirsql.toml")).toEqual({
      entries,
      base: "/cfg/nested",
    });
  });

  it("reads the config as utf8 text and parses that text", () => {
    vi.mocked(readFileSync).mockReturnValue("[dirsql]\n");
    stubConfig({ dirsql: { extension: [] } });
    expect(loadExtensionEntries("/cfg/.dirsql.toml")).toEqual({
      entries: [],
      base: "/cfg",
    });
    expect(readFileSync).toHaveBeenCalledWith("/cfg/.dirsql.toml", "utf8");
    expect(parseToml).toHaveBeenCalledWith("[dirsql]\n");
  });

  it("resolves a relative config path against the cwd", () => {
    stubConfig({ dirsql: { extension: [] } });
    const loaded = loadExtensionEntries(".dirsql.toml");
    expect(loaded?.base).toBe(process.cwd());
  });
});
