import { describe, expect, it, vi } from "vitest";
import { loadApp } from "./loadApp.js";

describe("loadApp", () => {
  it("returns the default export the importer yields", async () => {
    const importer = vi
      .fn()
      .mockResolvedValue({ default: { sentinel: "value" } });
    const app = (await loadApp("/cfg/config.mjs", importer)) as unknown as {
      sentinel: string;
    };
    expect(app.sentinel).toBe("value");
    // The importer receives the file:// URL form of the config path.
    expect(importer).toHaveBeenCalledWith(
      expect.stringMatching(/^file:.*config\.mjs$/),
    );
  });

  it("throws a path-aware error when the module has no default export", async () => {
    const importer = vi.fn().mockResolvedValue({ x: 1 });
    await expect(loadApp("/cfg/no_default.mjs", importer)).rejects.toThrow(
      /must default-export a DirSQL instance/,
    );
    await expect(loadApp("/cfg/no_default.mjs", importer)).rejects.toThrow(
      "/cfg/no_default.mjs",
    );
  });

  it("propagates errors thrown during module evaluation", async () => {
    const importer = vi.fn().mockRejectedValue(new Error("synthetic boom"));
    await expect(loadApp("/cfg/boom.mjs", importer)).rejects.toThrow(
      /synthetic boom/,
    );
  });

  it("rejects when the importer rejects (missing file)", async () => {
    const importer = vi.fn().mockRejectedValue(new Error("Cannot find module"));
    await expect(loadApp("/cfg/nope.mjs", importer)).rejects.toThrow();
  });

  it("defaults to the real dynamic import when no importer is injected", async () => {
    // Exercises the default `importer` arg (a real `import()`). The path
    // doesn't exist, so the underlying import rejects -- which is all we
    // need to drive the default callback's single line.
    await expect(loadApp("/no/such/dirsql.config.mjs")).rejects.toThrow();
  });
});
