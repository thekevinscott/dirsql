import { describe, expect, it } from "vitest";
import type { TableDef } from "../../index.js";
import { dispatchExtract } from "./dispatch-extract.js";

function tableMap(
  entries: Array<[string, TableDef["extract"]]>,
): Map<string, TableDef> {
  return new Map(
    entries.map(([n, extract]) => [
      n,
      {
        ddl: `CREATE TABLE ${n} (x TEXT)`,
        glob: "*",
        extract,
      },
    ]),
  );
}

describe("dispatchExtract", () => {
  it("returns ok=true with the rows the sync extract returned", async () => {
    const tables = tableMap([["papers", (p) => [{ row: p }]]]);
    const out = await dispatchExtract(
      { type: "extract", id: 1, table: "papers", path: "/a" },
      tables,
    );
    expect(out).toEqual({
      type: "result",
      id: 1,
      ok: true,
      rows: [{ row: "/a" }],
    });
  });

  it("echoes the request id verbatim on success", async () => {
    const tables = tableMap([["t", () => []]]);
    const out = await dispatchExtract(
      { type: "extract", id: 99, table: "t", path: "/" },
      tables,
    );
    expect(out.id).toBe(99);
  });

  it("returns ok=false with a JSON-encoded name when the table is unknown", async () => {
    const out = await dispatchExtract(
      { type: "extract", id: 5, table: "ghost", path: "/" },
      tableMap([]),
    );
    expect(out.ok).toBe(false);
    expect(out.error).toContain('"ghost"');
  });

  it("echoes the request id verbatim on unknown-table failure", async () => {
    const out = await dispatchExtract(
      { type: "extract", id: 17, table: "ghost", path: "/" },
      tableMap([]),
    );
    expect(out.id).toBe(17);
  });

  it("returns ok=false with the message when a sync extract throws", async () => {
    const tables = tableMap([
      [
        "papers",
        () => {
          throw new Error("synthetic");
        },
      ],
    ]);
    const out = await dispatchExtract(
      { type: "extract", id: 7, table: "papers", path: "/" },
      tables,
    );
    expect(out).toEqual({
      type: "result",
      id: 7,
      ok: false,
      error: "synthetic",
    });
  });

  it("treats a missing table field as unknown table", async () => {
    const out = await dispatchExtract(
      { type: "extract", id: 1, path: "/" },
      tableMap([["t", () => []]]),
    );
    expect(out.ok).toBe(false);
    expect(out.error).toMatch(/unknown table/);
  });

  it("passes the request path to the extract callback", async () => {
    const seen: string[] = [];
    const tables = tableMap([
      [
        "t",
        (p) => {
          seen.push(p);
          return [];
        },
      ],
    ]);
    await dispatchExtract(
      { type: "extract", id: 1, table: "t", path: "/abs/x.json" },
      tables,
    );
    expect(seen).toEqual(["/abs/x.json"]);
  });
});
