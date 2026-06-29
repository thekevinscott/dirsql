// Unit tests for `resolveConfig`.

import { readFileSync } from "node:fs";
import { describe, expect, it, vi } from "vitest";
import { resolveConfig } from "./resolve-config.js";

// `resolveConfig`'s only effectful collaborator is `readFileSync`; `node:path`
// and `smol-toml` are pure. Mock fs so the test never touches a real file --
// each "with a config file" case stubs the TOML body it wants to parse.
vi.mock("node:fs", async () => ({
  ...(await vi.importActual<typeof import("node:fs")>("node:fs")),
  readFileSync: vi.fn(),
}));

const noopExtract = () => [];

// Fixed posix config path. CI is linux, so `dirname(resolve(cfgPath))` is
// deterministically `/cfg` -- relative config paths resolve against it.
const cfgPath = "/cfg/.dirsql.toml";
const cfgDir = "/cfg";

describe("resolveConfig", () => {
  const writeCfg = (body: string) => {
    vi.mocked(readFileSync).mockReturnValue(body);
  };

  describe("without a config file", () => {
    it("forwards the root kwarg, defaults everything else", () => {
      expect(resolveConfig({ root: "/abs/data" })).toEqual({
        root: "/abs/data",
        tables: [],
        ignore: [],
        persist: false,
        persistPath: null,
        extensions: [],
      });
    });

    it("maps programmatic extensions verbatim, normalizing entrypoint to null", () => {
      const out = resolveConfig({
        root: "/r",
        extensions: [
          { path: "/ext/vec0.so", entrypoint: "sqlite3_vec_init" },
          { path: "rel/spellfix.so" },
        ],
      });
      expect(out.extensions).toEqual([
        { path: "/ext/vec0.so", entrypoint: "sqlite3_vec_init" },
        // Programmatic paths are NOT resolved (verbatim, mirroring Rust).
        { path: "rel/spellfix.so", entrypoint: null },
      ]);
    });

    it("normalizes programmatic strict: undefined to false", () => {
      const out = resolveConfig({
        root: "/r",
        tables: [{ ddl: "x", glob: "*", extract: noopExtract }],
      });
      expect(out.tables).toEqual([{ ddl: "x", glob: "*", strict: false }]);
    });

    it("normalizes programmatic strict: true to true", () => {
      const out = resolveConfig({
        root: "/r",
        tables: [{ ddl: "x", glob: "*", extract: noopExtract, strict: true }],
      });
      expect(out.tables).toEqual([{ ddl: "x", glob: "*", strict: true }]);
    });

    it("normalizes programmatic strict: false to false", () => {
      const out = resolveConfig({
        root: "/r",
        tables: [{ ddl: "x", glob: "*", extract: noopExtract, strict: false }],
      });
      expect(out.tables).toEqual([{ ddl: "x", glob: "*", strict: false }]);
    });

    it("forwards the ignore kwarg", () => {
      const out = resolveConfig({ root: "/r", ignore: ["**/skip/**"] });
      expect(out.ignore).toEqual(["**/skip/**"]);
    });

    it("forwards persist + persistPath kwargs", () => {
      const out = resolveConfig({
        root: "/r",
        persist: true,
        persistPath: "/abs/cache.db",
      });
      expect(out.persist).toBe(true);
      expect(out.persistPath).toBe("/abs/cache.db");
    });
  });

  describe("with a config file", () => {
    it("resolves a relative [dirsql].root against the config file's parent", () => {
      writeCfg('[dirsql]\nroot = "data"\n');
      const out = resolveConfig({ config: cfgPath });
      expect(out.root).toBe("/cfg/data");
    });

    it("preserves an absolute [dirsql].root verbatim", () => {
      writeCfg('[dirsql]\nroot = "/other/abs/path"\n');
      const out = resolveConfig({ config: cfgPath });
      expect(out.root).toBe("/other/abs/path");
    });

    it("defaults the root to the config file's parent when [dirsql].root is absent", () => {
      writeCfg('[dirsql]\nignore = ["x"]\n');
      const out = resolveConfig({ config: cfgPath });
      expect(out.root).toBe(cfgDir);
    });

    it("defaults the root to the config file's parent when [dirsql] is absent entirely", () => {
      writeCfg('[[table]]\nddl = "CREATE TABLE t (x TEXT)"\nglob = "*.json"\n');
      const out = resolveConfig({ config: cfgPath });
      expect(out.root).toBe(cfgDir);
    });

    it("handles a non-string [dirsql].root by falling back to the config dir", () => {
      writeCfg("[dirsql]\nroot = 42\n");
      const out = resolveConfig({ config: cfgPath });
      expect(out.root).toBe(cfgDir);
    });

    it("reads [[table]] entries with strict defaulting to false", () => {
      writeCfg('[[table]]\nddl = "CREATE TABLE t (x TEXT)"\nglob = "*.json"\n');
      const out = resolveConfig({ config: cfgPath });
      expect(out.tables).toEqual([
        { ddl: "CREATE TABLE t (x TEXT)", glob: "*.json", strict: false },
      ]);
    });

    it("respects strict = true on a [[table]] entry", () => {
      writeCfg(
        '[[table]]\nddl = "CREATE TABLE t (x TEXT)"\nglob = "*.json"\nstrict = true\n',
      );
      const out = resolveConfig({ config: cfgPath });
      expect(out.tables).toEqual([
        { ddl: "CREATE TABLE t (x TEXT)", glob: "*.json", strict: true },
      ]);
    });

    it("returns an empty tables list when no [[table]] entries are declared", () => {
      writeCfg("[dirsql]\nignore = []\n");
      const out = resolveConfig({ config: cfgPath });
      expect(out.tables).toEqual([]);
    });

    it("returns an empty tables list when `table` exists but isn't an array", () => {
      // TOML's `[table]` (singular table header) yields a non-array value
      // for the same key name. resolveConfig must defensively coerce.
      writeCfg('[table]\nddl = "CREATE TABLE t (x TEXT)"\nglob = "*.json"\n');
      const out = resolveConfig({ config: cfgPath });
      expect(out.tables).toEqual([]);
    });

    it("forwards [dirsql].ignore", () => {
      writeCfg('[dirsql]\nignore = ["node_modules/**"]\n');
      const out = resolveConfig({ config: cfgPath });
      expect(out.ignore).toEqual(["node_modules/**"]);
    });

    it("flips persist on when [dirsql].persist = true", () => {
      writeCfg("[dirsql]\npersist = true\n");
      const out = resolveConfig({ config: cfgPath });
      expect(out.persist).toBe(true);
    });

    it("leaves persist off when [dirsql].persist is absent", () => {
      writeCfg('[dirsql]\nignore = ["x"]\n');
      const out = resolveConfig({ config: cfgPath });
      expect(out.persist).toBe(false);
    });

    it("resolves a relative [dirsql].persist_path against the config dir", () => {
      writeCfg('[dirsql]\npersist = true\npersist_path = "cache/dirsql.db"\n');
      const out = resolveConfig({ config: cfgPath });
      expect(out.persistPath).toBe("/cfg/cache/dirsql.db");
    });

    it("preserves an absolute [dirsql].persist_path verbatim", () => {
      writeCfg('[dirsql]\npersist_path = "/var/cache/dirsql.db"\n');
      const out = resolveConfig({ config: cfgPath });
      expect(out.persistPath).toBe("/var/cache/dirsql.db");
    });

    it("leaves persistPath null when [dirsql].persist_path is absent", () => {
      writeCfg('[dirsql]\nignore = ["x"]\n');
      const out = resolveConfig({ config: cfgPath });
      expect(out.persistPath).toBeNull();
    });

    it("ignores a non-string [dirsql].persist_path", () => {
      writeCfg("[dirsql]\npersist_path = 42\n");
      const out = resolveConfig({ config: cfgPath });
      expect(out.persistPath).toBeNull();
    });

    it("defaults extensions to [] when no [[dirsql.extension]] entries exist", () => {
      writeCfg('[dirsql]\nignore = ["x"]\n');
      const out = resolveConfig({ config: cfgPath });
      expect(out.extensions).toEqual([]);
    });

    it("resolves a relative [[dirsql.extension]].path against the config dir", () => {
      writeCfg(
        '[[dirsql.extension]]\npath = "ext/vec0.so"\nentrypoint = "sqlite3_vec_init"\n',
      );
      const out = resolveConfig({ config: cfgPath });
      expect(out.extensions).toEqual([
        { path: "/cfg/ext/vec0.so", entrypoint: "sqlite3_vec_init" },
      ]);
    });

    it("preserves an absolute [[dirsql.extension]].path and defaults entrypoint to null", () => {
      writeCfg('[[dirsql.extension]]\npath = "/abs/spellfix.so"\n');
      const out = resolveConfig({ config: cfgPath });
      expect(out.extensions).toEqual([
        { path: "/abs/spellfix.so", entrypoint: null },
      ]);
    });
  });

  describe("merging kwargs with a config file", () => {
    it("kwarg root wins over [dirsql].root", () => {
      writeCfg('[dirsql]\nroot = "from-config"\n');
      const out = resolveConfig({ root: "/from-kwarg", config: cfgPath });
      expect(out.root).toBe("/from-kwarg");
    });

    it("concatenates programmatic tables then config tables, in order", () => {
      writeCfg('[[table]]\nddl = "CREATE TABLE c (x TEXT)"\nglob = "c/*"\n');
      const out = resolveConfig({
        root: "/r",
        tables: [{ ddl: "p-ddl", glob: "p/*", extract: noopExtract }],
        config: cfgPath,
      });
      expect(out.tables.map((t) => t.ddl)).toEqual([
        "p-ddl",
        "CREATE TABLE c (x TEXT)",
      ]);
    });

    it("concatenates ignore kwargs first, then config ignore", () => {
      writeCfg('[dirsql]\nignore = ["from-config/**"]\n');
      const out = resolveConfig({
        root: "/r",
        ignore: ["from-kwarg/**"],
        config: cfgPath,
      });
      expect(out.ignore).toEqual(["from-kwarg/**", "from-config/**"]);
    });

    it("OR-s persist across kwarg and config (kwarg true, config absent)", () => {
      writeCfg('[dirsql]\nignore = ["x"]\n');
      const out = resolveConfig({
        root: "/r",
        persist: true,
        config: cfgPath,
      });
      expect(out.persist).toBe(true);
    });

    it("OR-s persist across kwarg and config (kwarg absent, config true)", () => {
      writeCfg("[dirsql]\npersist = true\n");
      const out = resolveConfig({ root: "/r", config: cfgPath });
      expect(out.persist).toBe(true);
    });

    it("concatenates programmatic extensions first, then config extensions", () => {
      writeCfg('[[dirsql.extension]]\npath = "from-config.so"\n');
      const out = resolveConfig({
        root: "/r",
        extensions: [{ path: "/prog/from-kwarg.so", entrypoint: "init_kw" }],
        config: cfgPath,
      });
      expect(out.extensions).toEqual([
        { path: "/prog/from-kwarg.so", entrypoint: "init_kw" },
        { path: "/cfg/from-config.so", entrypoint: null },
      ]);
    });

    it("kwarg persistPath wins over config persist_path", () => {
      writeCfg('[dirsql]\npersist_path = "from-config.db"\n');
      const out = resolveConfig({
        root: "/r",
        persistPath: "/from-kwarg.db",
        config: cfgPath,
      });
      expect(out.persistPath).toBe("/from-kwarg.db");
    });
  });
});
