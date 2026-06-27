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
import { buildTables } from "./buildTables.js";
import { dispatchExtract } from "./dispatchExtract.js";
import { interpret } from "./interpret.js";
import { loadApp } from "./loadApp.js";
import { writeMessage } from "./writeMessage.js";

// Every collaborator is mocked so the test isolates `interpret`'s glue logic.
// `node:readline`'s `createInterface` is faked to return an async iterable of
// lines directly -- this stands in for the line-buffered read over stdin, so
// the test never needs a real `Readable` stream.
vi.mock("./buildTables.js");
vi.mock("./dispatchExtract.js");
vi.mock("./loadApp.js");
vi.mock("./writeMessage.js");
vi.mock("node:readline", async () => ({
  ...(await vi.importActual<typeof import("node:readline")>("node:readline")),
  createInterface: vi.fn(),
}));

// Imported after the mock so we get the mocked module.
const { createInterface } = await import("node:readline");

function fakeApp(overrides: Partial<DirSQL> = {}): DirSQL {
  const app = {
    ready: Promise.resolve(),
    toJSON: () => ({ root: "/x" }),
    _options: { tables: [] },
    ...overrides,
  } as unknown as DirSQL;
  return app;
}

// Fake the readline interface as an async iterable yielding the given lines.
function stubLines(lines: string[]): void {
  vi.mocked(createInterface).mockReturnValue(
    (async function* () {
      for (const l of lines) {
        yield l;
      }
      // biome-ignore lint/suspicious/noExplicitAny: minimal readline stub
    })() as any,
  );
}

describe("interpret", () => {
  let stderrWrite: Mock;

  beforeEach(() => {
    stderrWrite = vi.fn();
    vi.stubGlobal("process", {
      ...process,
      stderr: { write: stderrWrite },
    });
    stubLines([]);
    vi.mocked(writeMessage).mockImplementation(() => {});
    vi.mocked(buildTables).mockReturnValue(new Map());
    vi.mocked(dispatchExtract).mockResolvedValue({
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
      vi.mocked(loadApp).mockRejectedValue(new Error("no app"));
      expect(await interpret("bad.mjs")).toBe(1);
      expect(stderrWrite).toHaveBeenCalledExactlyOnceWith(
        "dirsql interpret: no app\n",
      );
    });

    it("coerces non-Error throws via String() in the stderr message", async () => {
      vi.mocked(loadApp).mockRejectedValue("plain string");
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
      vi.mocked(loadApp).mockResolvedValue(app);
      await interpret("good.mjs");
      expect(writeMessage).toHaveBeenCalledWith({
        type: "config",
        state: { root: "/here", tables: [], ignore: [], persist: false },
      });
    });

    it("attaches a no-op catch to app.ready to swallow background-scan failures", async () => {
      // Verify the structural property -- interpret CALLS `.catch` on
      // `app.ready` -- without exercising the runtime side effect
      // (which fights vitest's own unhandled-rejection handling).
      const catchSpy = vi.fn(() => Promise.resolve());
      const app = fakeApp({
        ready: { catch: catchSpy } as unknown as Promise<void>,
      });
      vi.mocked(loadApp).mockResolvedValue(app);

      await interpret("good.mjs");
      expect(catchSpy).toHaveBeenCalledOnce();
    });
  });

  describe("extract loop", () => {
    it("dispatches one extract request and writes the response", async () => {
      vi.mocked(loadApp).mockResolvedValue(fakeApp());
      const req = { type: "extract", id: 1, table: "t", path: "/a" };
      stubLines([JSON.stringify(req)]);

      const expected = {
        type: "result" as const,
        id: 1,
        ok: true,
        rows: [{ row: "/a" }],
      };
      vi.mocked(dispatchExtract).mockResolvedValue(expected);

      expect(await interpret("good.mjs")).toBe(0);
      expect(dispatchExtract).toHaveBeenCalledWith(req, new Map());
      // handshake first, then response
      expect(writeMessage).toHaveBeenCalledTimes(2);
      expect(writeMessage).toHaveBeenLastCalledWith(expected);
    });

    it("skips blank lines silently", async () => {
      vi.mocked(loadApp).mockResolvedValue(fakeApp());
      stubLines(["", "   "]);
      expect(await interpret("good.mjs")).toBe(0);
      expect(dispatchExtract).not.toHaveBeenCalled();
      // handshake only
      expect(writeMessage).toHaveBeenCalledOnce();
    });

    it("skips malformed JSON silently", async () => {
      vi.mocked(loadApp).mockResolvedValue(fakeApp());
      stubLines(["not json", "{also bad"]);
      expect(await interpret("good.mjs")).toBe(0);
      expect(dispatchExtract).not.toHaveBeenCalled();
    });

    it("skips non-extract messages silently", async () => {
      vi.mocked(loadApp).mockResolvedValue(fakeApp());
      stubLines([JSON.stringify({ type: "ping" })]);
      expect(await interpret("good.mjs")).toBe(0);
      expect(dispatchExtract).not.toHaveBeenCalled();
    });

    it("skips a null / non-object JSON payload silently", async () => {
      vi.mocked(loadApp).mockResolvedValue(fakeApp());
      stubLines(["null", "42", "[]"]);
      expect(await interpret("good.mjs")).toBe(0);
      expect(dispatchExtract).not.toHaveBeenCalled();
    });

    it("returns 0 when stdin closes", async () => {
      vi.mocked(loadApp).mockResolvedValue(fakeApp());
      expect(await interpret("good.mjs")).toBe(0);
    });
  });
});
