import { Readable } from "node:stream";
import {
  type Mock,
  afterEach,
  beforeEach,
  describe,
  expect,
  it,
  vi,
} from "vitest";
import type { DirSQL } from "../../index.js";
import * as buildTablesMod from "./buildTables.js";
import * as dispatchMod from "./dispatchExtract.js";
import { interpret } from "./interpret.js";
import * as loadAppMod from "./loadApp.js";
import * as writeMod from "./writeMessage.js";

function fakeApp(overrides: Partial<DirSQL> = {}): DirSQL {
  const app = {
    ready: Promise.resolve(),
    toJSON: () => ({ root: "/x" }),
    _options: { tables: [] },
    ...overrides,
  } as unknown as DirSQL;
  return app;
}

function asLines(lines: string[]): Readable {
  return Readable.from(lines.map((l) => `${l}\n`));
}

describe("interpret", () => {
  let stderrWrite: Mock;

  beforeEach(() => {
    stderrWrite = vi.fn();
    vi.stubGlobal("process", {
      ...process,
      stdin: asLines([]),
      stderr: { write: stderrWrite },
    });
    vi.spyOn(writeMod, "writeMessage").mockImplementation(() => {});
    vi.spyOn(buildTablesMod, "buildTables").mockReturnValue(new Map());
    vi.spyOn(dispatchMod, "dispatchExtract").mockResolvedValue({
      type: "result",
      id: 1,
      ok: true,
      rows: [],
    });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  describe("startup", () => {
    it("returns 1 and writes to stderr when configPath is empty", async () => {
      expect(await interpret("")).toBe(1);
      expect(stderrWrite).toHaveBeenCalledExactlyOnceWith(
        "dirsql interpret: expected one config path, got 0\n",
      );
    });

    it("returns 1 and writes a dirsql-prefixed line when loadApp throws", async () => {
      vi.spyOn(loadAppMod, "loadApp").mockRejectedValue(new Error("no app"));
      expect(await interpret("bad.mjs")).toBe(1);
      expect(stderrWrite).toHaveBeenCalledExactlyOnceWith(
        "dirsql interpret: no app\n",
      );
    });

    it("coerces non-Error throws via String() in the stderr message", async () => {
      vi.spyOn(loadAppMod, "loadApp").mockRejectedValue("plain string");
      expect(await interpret("bad.mjs")).toBe(1);
      expect(stderrWrite).toHaveBeenCalledExactlyOnceWith(
        "dirsql interpret: plain string\n",
      );
    });
  });

  describe("handshake", () => {
    it("writes one config message with app.toJSON() as state", async () => {
      const app = fakeApp({
        toJSON: () =>
          ({ root: "/here", tables: [], ignore: [], persist: false }) as never,
      });
      vi.spyOn(loadAppMod, "loadApp").mockResolvedValue(app);
      await interpret("good.mjs");
      expect(writeMod.writeMessage).toHaveBeenCalledWith({
        type: "config",
        state: { root: "/here", tables: [], ignore: [], persist: false },
      });
    });

    it("attaches a no-op catch to app.ready so the scan rejection cannot crash node", async () => {
      const scanError = new Error("scan exploded");
      const ready = Promise.reject(scanError);
      const app = fakeApp({ ready });
      vi.spyOn(loadAppMod, "loadApp").mockResolvedValue(app);

      const unhandled = vi.fn();
      process.on("unhandledRejection", unhandled);
      try {
        await interpret("good.mjs");
        await new Promise((r) => setImmediate(r));
        expect(unhandled).not.toHaveBeenCalled();
      } finally {
        process.off("unhandledRejection", unhandled);
      }
    });
  });

  describe("extract loop", () => {
    it("dispatches one extract request and writes the response", async () => {
      vi.spyOn(loadAppMod, "loadApp").mockResolvedValue(fakeApp());
      const req = { type: "extract", id: 1, table: "t", path: "/a" };
      vi.stubGlobal("process", {
        ...process,
        stdin: asLines([JSON.stringify(req)]),
        stderr: { write: stderrWrite },
      });

      const expected = {
        type: "result" as const,
        id: 1,
        ok: true,
        rows: [{ row: "/a" }],
      };
      vi.mocked(dispatchMod.dispatchExtract).mockResolvedValue(expected);

      expect(await interpret("good.mjs")).toBe(0);
      expect(dispatchMod.dispatchExtract).toHaveBeenCalledWith(req, new Map());
      // handshake first, then response
      expect(writeMod.writeMessage).toHaveBeenCalledTimes(2);
      expect(writeMod.writeMessage).toHaveBeenLastCalledWith(expected);
    });

    it("skips blank lines silently", async () => {
      vi.spyOn(loadAppMod, "loadApp").mockResolvedValue(fakeApp());
      vi.stubGlobal("process", {
        ...process,
        stdin: asLines(["", "   "]),
        stderr: { write: stderrWrite },
      });
      expect(await interpret("good.mjs")).toBe(0);
      expect(dispatchMod.dispatchExtract).not.toHaveBeenCalled();
      // handshake only
      expect(writeMod.writeMessage).toHaveBeenCalledOnce();
    });

    it("skips malformed JSON silently", async () => {
      vi.spyOn(loadAppMod, "loadApp").mockResolvedValue(fakeApp());
      vi.stubGlobal("process", {
        ...process,
        stdin: asLines(["not json", "{also bad"]),
        stderr: { write: stderrWrite },
      });
      expect(await interpret("good.mjs")).toBe(0);
      expect(dispatchMod.dispatchExtract).not.toHaveBeenCalled();
    });

    it("skips non-extract messages silently", async () => {
      vi.spyOn(loadAppMod, "loadApp").mockResolvedValue(fakeApp());
      vi.stubGlobal("process", {
        ...process,
        stdin: asLines([JSON.stringify({ type: "ping" })]),
        stderr: { write: stderrWrite },
      });
      expect(await interpret("good.mjs")).toBe(0);
      expect(dispatchMod.dispatchExtract).not.toHaveBeenCalled();
    });

    it("skips a null / non-object JSON payload silently", async () => {
      vi.spyOn(loadAppMod, "loadApp").mockResolvedValue(fakeApp());
      vi.stubGlobal("process", {
        ...process,
        stdin: asLines(["null", "42", "[]"]),
        stderr: { write: stderrWrite },
      });
      expect(await interpret("good.mjs")).toBe(0);
      expect(dispatchMod.dispatchExtract).not.toHaveBeenCalled();
    });

    it("returns 0 when stdin closes", async () => {
      vi.spyOn(loadAppMod, "loadApp").mockResolvedValue(fakeApp());
      expect(await interpret("good.mjs")).toBe(0);
    });
  });
});
