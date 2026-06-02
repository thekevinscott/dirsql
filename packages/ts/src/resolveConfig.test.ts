// Unit tests for `resolveConfig`.

import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { isAbsolute, join } from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { resolveConfig } from "./resolveConfig.js";

const noopExtract = () => [];

describe("resolveConfig", () => {
  let dir: string;
  let cfgPath: string;

  beforeEach(() => {
    dir = mkdtempSync(join(tmpdir(), "dirsql-resolveConfig-"));
    cfgPath = join(dir, ".dirsql.toml");
  });

  afterEach(() => {
    rmSync(dir, { recursive: true, force: true });
  });

  const writeCfg = (body: string) => {
    writeFileSync(cfgPath, body);
  };

  describe("without a config file", () => {
    it("forwards the root kwarg, defaults everything else", () => {
      expect(resolveConfig({ root: "/abs/data" })).toEqual({
        root: "/abs/data",
        tables: [],
        ignore: [],
        persist: false,
        persistPath: null,
      });
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
      expect(out.root).toBe(join(dir, "data"));
      expect(isAbsolute(out.root)).toBe(true);
    });

    it("preserves an absolute [dirsql].root verbatim", () => {
      writeCfg('[dirsql]\nroot = "/other/abs/path"\n');
      const out = resolveConfig({ config: cfgPath });
      expect(out.root).toBe("/other/abs/path");
    });

    it("defaults the root to the config file's parent when [dirsql].root is absent", () => {
      writeCfg('[dirsql]\nignore = ["x"]\n');
      const out = resolveConfig({ config: cfgPath });
      expect(out.root).toBe(dir);
    });

    it("defaults the root to the config file's parent when [dirsql] is absent entirely", () => {
      writeCfg('[[table]]\nddl = "CREATE TABLE t (x TEXT)"\nglob = "*.json"\n');
      const out = resolveConfig({ config: cfgPath });
      expect(out.root).toBe(dir);
    });

    it("handles a non-string [dirsql].root by falling back to the config dir", () => {
      writeCfg("[dirsql]\nroot = 42\n");
      const out = resolveConfig({ config: cfgPath });
      expect(out.root).toBe(dir);
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
      expect(out.persistPath).toBe(join(dir, "cache/dirsql.db"));
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
