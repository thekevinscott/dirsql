import { describe, it, expect } from "vitest";
import { diff } from "./differ.js";
import type { Row } from "./db.js";

function row(pairs: Record<string, string | number | null>): Row {
  return { ...pairs };
}

describe("diff", () => {
  describe("file created (old is null)", () => {
    it("produces insert events for all rows", () => {
      const rows = [row({ name: "alice", age: 30 }), row({ name: "bob", age: 25 })];
      const events = diff("users", null, rows, "users.jsonl");
      expect(events).toHaveLength(2);
      expect(events[0]).toMatchObject({ action: "insert", table: "users" });
      expect(events[0].row).toMatchObject({ name: "alice" });
      expect(events[1].row).toMatchObject({ name: "bob" });
    });
  });

  describe("file deleted (new is null)", () => {
    it("produces delete events for all rows", () => {
      const rows = [row({ id: "1" }), row({ id: "2" })];
      const events = diff("items", rows, null, "items.jsonl");
      expect(events).toHaveLength(2);
      expect(events[0]).toMatchObject({ action: "delete", table: "items" });
      expect(events[0].row).toMatchObject({ id: "1" });
    });
  });

  describe("no changes", () => {
    it("produces no events when content is identical", () => {
      const rows = [row({ x: 1 }), row({ x: 2 })];
      const events = diff("t", rows, rows, "t.jsonl");
      expect(events).toHaveLength(0);
    });
  });

  describe("single line change", () => {
    it("produces update event for changed line", () => {
      const old = [row({ val: "a" }), row({ val: "b" }), row({ val: "c" })];
      const now = [row({ val: "a" }), row({ val: "B" }), row({ val: "c" })];
      const events = diff("t", old, now, "t.jsonl");
      expect(events).toHaveLength(1);
      expect(events[0]).toMatchObject({
        action: "update",
        oldRow: { val: "b" },
        row: { val: "B" },
      });
    });
  });

  describe("appended lines", () => {
    it("produces insert events for new lines", () => {
      const old = [row({ id: 1 })];
      const now = [row({ id: 1 }), row({ id: 2 }), row({ id: 3 })];
      const events = diff("t", old, now, "t.jsonl");
      expect(events).toHaveLength(2);
      expect(events[0]).toMatchObject({ action: "insert", row: { id: 2 } });
      expect(events[1]).toMatchObject({ action: "insert", row: { id: 3 } });
    });
  });

  describe("full replace on shrink", () => {
    it("does full replace when file shrinks", () => {
      const old = [row({ id: 1 }), row({ id: 2 }), row({ id: 3 })];
      const now = [row({ id: 1 })];
      const events = diff("t", old, now, "t.jsonl");
      const deletes = events.filter((e) => e.action === "delete");
      const inserts = events.filter((e) => e.action === "insert");
      expect(deletes).toHaveLength(3);
      expect(inserts).toHaveLength(1);
    });
  });

  describe("full replace on heavy modification", () => {
    it("does full replace when >50% changed", () => {
      const old = [
        row({ v: "a" }),
        row({ v: "b" }),
        row({ v: "c" }),
        row({ v: "d" }),
      ];
      const now = [
        row({ v: "A" }),
        row({ v: "B" }),
        row({ v: "C" }),
        row({ v: "d" }),
      ];
      const events = diff("t", old, now, "t.jsonl");
      const deletes = events.filter((e) => e.action === "delete");
      const inserts = events.filter((e) => e.action === "insert");
      expect(deletes).toHaveLength(4);
      expect(inserts).toHaveLength(4);
    });

    it("does NOT full replace when exactly 50% changed", () => {
      const old = [
        row({ v: "a" }),
        row({ v: "b" }),
        row({ v: "c" }),
        row({ v: "d" }),
      ];
      const now = [
        row({ v: "A" }),
        row({ v: "B" }),
        row({ v: "c" }),
        row({ v: "d" }),
      ];
      const events = diff("t", old, now, "t.jsonl");
      expect(events).toHaveLength(2);
      expect(events.every((e) => e.action === "update")).toBe(true);
    });
  });

  describe("single-row file", () => {
    it("produces update event", () => {
      const old = [row({ title: "Draft" })];
      const now = [row({ title: "Final" })];
      const events = diff("docs", old, now, "doc.json");
      expect(events).toHaveLength(1);
      expect(events[0]).toMatchObject({
        action: "update",
        oldRow: { title: "Draft" },
        row: { title: "Final" },
      });
    });

    it("produces no events when unchanged", () => {
      const rows = [row({ title: "Same" })];
      const events = diff("docs", rows, rows, "doc.json");
      expect(events).toHaveLength(0);
    });
  });

  describe("both null", () => {
    it("produces no events", () => {
      const events = diff("t", null, null, "gone.json");
      expect(events).toHaveLength(0);
    });
  });

  describe("full replace ordering", () => {
    it("deletes come before inserts", () => {
      const old = [row({ id: 1 }), row({ id: 2 })];
      const now = [row({ id: 3 })];
      const events = diff("t", old, now, "t.jsonl");
      const lastDelete = events.reduce(
        (acc, e, i) => (e.action === "delete" ? i : acc),
        -1,
      );
      const firstInsert = events.findIndex((e) => e.action === "insert");
      expect(lastDelete).toBeLessThan(firstInsert);
    });
  });
});
