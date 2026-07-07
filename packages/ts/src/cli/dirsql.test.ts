// The npm `bin` entry shim invokes `runCli()` at module load, so it must be
// mocked before the shim is imported.

import { describe, expect, it, vi } from "vitest";
import { runCli } from "./run-cli.js";

vi.mock("./run-cli.js");

describe("dirsql bin shim", () => {
  it("invokes runCli on import", async () => {
    await import("./dirsql.js");
    expect(runCli).toHaveBeenCalledOnce();
  });
});
