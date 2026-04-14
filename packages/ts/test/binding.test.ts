// Integration tests for the TypeScript SDK binding layer.
//
// These tests exercise the napi-rs binding glue (`packages/ts/ts/index.ts`)
// in isolation by installing a configurable fake `DirSQL` on the stub
// `dirsql.node.js` module *before* dynamically importing the SDK. The
// SDK's top-level `require("../dirsql.node")` then captures our fake.
//
// Real engine behaviour (scanning, SQL, watching) is covered by the
// Rust core's unit tests and the local-only e2e suite.

import { createRequire } from "node:module";
import { beforeAll, beforeEach, describe, expect, it } from "vitest";

const req = createRequire(import.meta.url);
const nativeStub = req("../dirsql.node.js") as { DirSQL: unknown };

const state = {
  ctorCalls: [] as Array<{
    root: string;
    tables: unknown[];
    ignore?: string[];
  }>,
  queryCalls: [] as string[],
  startWatcherCalls: [] as number[],
  pollEventsCalls: [] as number[],
  queryImpl: ((_sql: string) => [{ ok: 1 }]) as (sql: string) => unknown[],
  startWatcherImpl: (() => {}) as () => void,
  pollEventsImpl: ((_t: number) => []) as (t: number) => unknown[],
};

class FakeDirSQL {
  constructor(root: string, tables: unknown[], ignore?: string[]) {
    state.ctorCalls.push({ root, tables, ignore });
  }
  query(sql: string) {
    state.queryCalls.push(sql);
    return state.queryImpl(sql);
  }
  startWatcher() {
    state.startWatcherCalls.push(Date.now());
    return state.startWatcherImpl();
  }
  pollEvents(timeoutMs: number) {
    state.pollEventsCalls.push(timeoutMs);
    return state.pollEventsImpl(timeoutMs);
  }
}

// Install the fake BEFORE the SDK loads.
nativeStub.DirSQL = FakeDirSQL;

type SDK = typeof import("../ts/index");
let sdk: SDK;

beforeAll(async () => {
  sdk = await import("../ts/index");
});

const table: import("../ts/index").TableDef = {
  ddl: "CREATE TABLE t (name TEXT)",
  glob: "**/*.json",
  extract: () => [],
};

beforeEach(() => {
  state.ctorCalls.length = 0;
  state.queryCalls.length = 0;
  state.startWatcherCalls.length = 0;
  state.pollEventsCalls.length = 0;
  state.queryImpl = () => [{ ok: 1 }];
  state.startWatcherImpl = () => {};
  state.pollEventsImpl = () => [];
});

describe("TypeScript binding layer", () => {
  describe("constructor", () => {
    // Feature: `new DirSQL(root, tables, ignore?)`. See
    // docs/guide/tables.md, docs/guide/config.md, and
    // packages/ts/README.md.
    it("passes constructor args through to the native module", () => {
      new sdk.DirSQL("/tmp/root", [table]);
      expect(state.ctorCalls).toHaveLength(1);
      expect(state.ctorCalls[0].root).toBe("/tmp/root");
      expect(state.ctorCalls[0].tables).toEqual([table]);
      expect(state.ctorCalls[0].ignore).toBeUndefined();
    });

    it("forwards the ignore list when provided", () => {
      // Feature: ignore patterns. docs/guide/tables.md,
      // packages/ts/README.md.
      const ignore = ["node_modules", ".git"];
      new sdk.DirSQL("/tmp/root", [table], ignore);
      expect(state.ctorCalls[0].ignore).toEqual(ignore);
    });
  });

  describe("query", () => {
    // Feature: db.query(sql). See docs/guide/querying.md,
    // packages/ts/README.md.
    it("delegates query() to the native instance", () => {
      state.queryImpl = () => [{ name: "Alice" }];
      const db = new sdk.DirSQL("/tmp/root", [table]);
      const rows = db.query("SELECT * FROM t");
      expect(state.queryCalls).toEqual(["SELECT * FROM t"]);
      expect(rows).toEqual([{ name: "Alice" }]);
    });

    it("propagates query errors from the native module", () => {
      state.queryImpl = () => {
        throw new Error("no such table");
      };
      const db = new sdk.DirSQL("/tmp/root", [table]);
      expect(() => db.query("SELECT * FROM missing")).toThrow("no such table");
    });
  });

  describe("watcher", () => {
    // Feature: startWatcher() + pollEvents(timeoutMs). See
    // docs/guide/watching.md and packages/ts/README.md.
    it("delegates startWatcher() to the native instance", () => {
      const db = new sdk.DirSQL("/tmp/root", [table]);
      db.startWatcher();
      expect(state.startWatcherCalls).toHaveLength(1);
    });

    it("delegates pollEvents() and passes the timeout through", () => {
      state.pollEventsImpl = () => [
        { table: "t", action: "insert", row: { name: "a" } },
      ];
      const db = new sdk.DirSQL("/tmp/root", [table]);
      const events = db.pollEvents(250);
      expect(state.pollEventsCalls).toEqual([250]);
      expect(events).toHaveLength(1);
      expect(events[0].action).toBe("insert");
    });

    it("propagates watcher errors from the native module", () => {
      state.startWatcherImpl = () => {
        throw new Error("watcher failed");
      };
      const db = new sdk.DirSQL("/tmp/root", [table]);
      expect(() => db.startWatcher()).toThrow("watcher failed");
    });
  });
});
