import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { keepSignalsFatal } from "./keep-signals-fatal.js";

describe("keepSignalsFatal", () => {
  let exit: ReturnType<typeof vi.fn>;
  let on: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    exit = vi.fn().mockImplementation((code: number) => {
      throw new Error(`EXIT_${code}`);
    });
    on = vi.fn();
    vi.stubGlobal("process", { ...process, exit, on });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.resetAllMocks();
  });

  it("registers a listener for SIGINT then SIGTERM", () => {
    on.mockImplementation((signal: string) => signal);

    keepSignalsFatal();

    expect(on.mock.calls.map((call) => call[0])).toEqual(["SIGINT", "SIGTERM"]);
  });

  it("exits 130 on SIGINT and 143 on SIGTERM", () => {
    const handlers: Record<string, () => void> = {};
    on.mockImplementation((signal: string, handler: () => void) => {
      handlers[signal] = handler;
    });

    keepSignalsFatal();

    expect(() => handlers.SIGINT?.()).toThrow("EXIT_130");
    expect(() => handlers.SIGTERM?.()).toThrow("EXIT_143");
  });
});
