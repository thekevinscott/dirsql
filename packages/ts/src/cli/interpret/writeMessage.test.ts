import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { writeMessage } from "./writeMessage.js";

describe("writeMessage", () => {
  let write: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    write = vi.fn();
    vi.stubGlobal("process", { ...process, stdout: { write } });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("writes one JSON line per call", () => {
    writeMessage({ type: "config", state: {} });
    expect(write).toHaveBeenCalledExactlyOnceWith(
      '{"type":"config","state":{}}\n',
    );
  });

  it("appends a trailing newline", () => {
    writeMessage({ x: 1 });
    expect((write.mock.calls[0]?.[0] as string).endsWith("\n")).toBe(true);
  });

  it("serializes nested arrays and objects deterministically", () => {
    writeMessage({ rows: [{ a: 1 }, { a: 2 }] });
    expect(write).toHaveBeenCalledExactlyOnceWith(
      '{"rows":[{"a":1},{"a":2}]}\n',
    );
  });
});
