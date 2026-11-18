// Binding-tier test that a build's JS heap does not grow with its file count:
// what one `onFile` call allocated must be collectable once the next one runs.
import { mkdirSync, writeFileSync } from "node:fs";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { setFlagsFromString } from "node:v8";
import { runInNewContext } from "node:vm";
import { DirSQL } from "dirsql";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

setFlagsFromString("--expose-gc");
const gc: () => void = runInNewContext("gc");

const SMALL = 5000;
const LARGE = SMALL * 4;
// Below this, a heap delta is collector noise rather than retained callbacks.
const NOISE_FLOOR = 1_000_000;

function writeTree(root: string, files: number): void {
  for (let i = 0; i < files; i++) {
    const sub = join(root, String(i % 50));
    mkdirSync(sub, { recursive: true });
    writeFileSync(join(sub, `f${i}.jsonl`), "{}\n");
  }
}

describe.each([false, true])("DirSQL build heap (persist: %s)", (persist) => {
  let dir: string;

  beforeEach(async () => {
    dir = await mkdtemp(join(tmpdir(), "dirsql-heap-"));
  });

  afterEach(async () => {
    await rm(dir, { recursive: true, force: true });
  });

  /** Heap retained, after a full collection, once every onFile has run. */
  const heapGrowth = async (files: number): Promise<number> => {
    const root = join(dir, String(files));
    writeTree(root, files);
    let calls = 0;
    let retained = 0;
    gc();
    const baseline = process.memoryUsage().heapUsed;
    const db = new DirSQL({
      root,
      persist,
      ...(persist && { persistPath: join(dir, `${files}.db`) }),
      tables: [
        {
          name: "t",
          ddl: "CREATE TABLE t (path, line, text)",
          glob: "**/*.jsonl",
          onFile: (path) => {
            calls++;
            if (calls === files) {
              gc();
              retained = process.memoryUsage().heapUsed - baseline;
            }
            return [{ path, line: 0, text: "x".repeat(200) }];
          },
        },
      ],
    });
    await db.ready;
    db.close();
    expect(calls).toBe(files);
    return Math.max(retained, NOISE_FLOOR);
  };

  it("keeps the heap flat as the file count grows", async () => {
    const small = await heapGrowth(SMALL);
    const large = await heapGrowth(LARGE);
    expect(large / small).toBeLessThan(2);
  }, 60_000);
});
