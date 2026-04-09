import { describe, it, expect } from "vitest";
import { TableMatcher } from "./matcher.js";

describe("TableMatcher", () => {
  describe("matchFile", () => {
    it("returns table name for matching glob", () => {
      const matcher = new TableMatcher([["*.csv", "data"]]);
      expect(matcher.matchFile("report.csv")).toBe("data");
    });

    it("returns null for non-matching path", () => {
      const matcher = new TableMatcher([["*.csv", "data"]]);
      expect(matcher.matchFile("readme.md")).toBeNull();
    });

    it("first matching pattern wins", () => {
      const matcher = new TableMatcher([
        ["*.json", "json_table"],
        ["data/*.json", "data_table"],
      ]);
      expect(matcher.matchFile("data/foo.json")).toBe("json_table");
    });

    it("matches nested paths with **", () => {
      const matcher = new TableMatcher([["**/*.jsonl", "events"]]);
      expect(matcher.matchFile("logs/2024/events.jsonl")).toBe("events");
    });
  });

  describe("isIgnored", () => {
    it("returns true for matching ignore pattern", () => {
      const matcher = new TableMatcher([], ["*.tmp", ".git/**"]);
      expect(matcher.isIgnored("scratch.tmp")).toBe(true);
      expect(matcher.isIgnored(".git/config")).toBe(true);
    });

    it("returns false for non-matching path", () => {
      const matcher = new TableMatcher([], ["*.tmp"]);
      expect(matcher.isIgnored("data.csv")).toBe(false);
    });
  });

  describe("empty matcher", () => {
    it("matches nothing", () => {
      const matcher = new TableMatcher([]);
      expect(matcher.matchFile("anything.txt")).toBeNull();
      expect(matcher.isIgnored("anything.txt")).toBe(false);
    });
  });
});
