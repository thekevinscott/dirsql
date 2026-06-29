import { describe, expect, it, vi } from "vitest";
import { getCore } from "./core.js";
import { parseTableName } from "./parse-table-name.js";

vi.mock("./core.js");

describe("parseTableName", () => {
  it("delegates to the core parser and returns its result", () => {
    const parse = vi.fn().mockReturnValue("users");
    vi.mocked(getCore).mockReturnValue({
      parseTableName: parse,
    } as unknown as ReturnType<typeof getCore>);

    expect(parseTableName("CREATE TABLE users (a INTEGER)")).toBe("users");
    expect(parse).toHaveBeenCalledWith("CREATE TABLE users (a INTEGER)");
  });

  it("propagates null for an unrecognized DDL", () => {
    const parse = vi.fn().mockReturnValue(null);
    vi.mocked(getCore).mockReturnValue({
      parseTableName: parse,
    } as unknown as ReturnType<typeof getCore>);

    expect(parseTableName("not ddl")).toBeNull();
  });
});
