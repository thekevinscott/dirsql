// The npm `bin` entry shim invokes `runCli()` at module load. Mock it so
// importing the shim is a no-op, then assert it was called (#239).

import { describe, expect, it, vi } from "vitest";
import { runCli } from "./run-cli.js";

vi.mock("./run-cli.js");

describe("dirsql bin shim", () => {
  it("invokes runCli on import", async () => {
    await import("./dirsql.js");
    expect(runCli).toHaveBeenCalledOnce();
  });
});
