// Single-request handler. Given a parsed extract request and the
// `name -> TableDef` map, call the user's extract callback (which may
// be sync or async) and build the NDJSON response payload the loop
// will write back.

import type { TableDef } from "../../index.js";
import { errMessage } from "./err-message.js";

export interface ExtractRequest {
  type?: string;
  id?: unknown;
  table?: string;
  path?: string;
}

export interface ResultMessage {
  type: "result";
  id: unknown;
  ok: boolean;
  rows?: unknown[];
  error?: string;
}

export async function dispatchExtract(
  req: ExtractRequest,
  tables: Map<string, TableDef>,
): Promise<ResultMessage> {
  const { id, table: name, path } = req;
  const table = name === undefined ? undefined : tables.get(name);
  if (!table) {
    return {
      type: "result",
      id,
      ok: false,
      error: `unknown table: ${JSON.stringify(name)}`,
    };
  }
  try {
    const rows = await table.extract(path as string);
    return { type: "result", id, ok: true, rows };
  } catch (e) {
    return { type: "result", id, ok: false, error: errMessage(e) };
  }
}
