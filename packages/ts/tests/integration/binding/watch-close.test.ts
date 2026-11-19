import { readFileSync } from "node:fs";
import { mkdtemp, rename, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { setFlagsFromString } from "node:v8";
import { runInNewContext } from "node:vm";
import { DirSQL } from "dirsql";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

const TEST_TIMEOUT = 15_000;

setFlagsFromString("--expose-gc");
const collectGarbage = runInNewContext("gc") as () => void;

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function itemsDb(root: string): DirSQL {
  return new DirSQL({
    root,
    tables: [
      {
        name: "items",
        ddl: "CREATE TABLE items (name TEXT)",
        glob: "**/*.json",
        onFile: (path: string) => [JSON.parse(readFileSync(path, "utf8"))],
      },
    ],
  });
}

async function writeAtomically(root: string, name: string): Promise<void> {
  const final = join(root, `${name}.json`);
  await writeFile(`${final}.tmp`, JSON.stringify({ name }));
  await rename(`${final}.tmp`, final);
}

async function firstInsert(
  stream: AsyncIterator<{
    action: string;
    row?: Record<string, unknown> | null;
  }>,
): Promise<unknown> {
  while (true) {
    const { value, done } = await stream.next();
    if (done) {
      return undefined;
    }
    if (value.action === "insert") {
      return value.row?.name;
    }
  }
}

async function indexed(db: DirSQL, name: string): Promise<void> {
  await expect
    .poll(() => db.query("SELECT name FROM items"), { timeout: 5000 })
    .toContainEqual({ name });
}

describe("closing a watch() stream", () => {
  let dir: string;

  beforeEach(async () => {
    dir = await mkdtemp(join(tmpdir(), "dirsql-watch-close-"));
  });

  afterEach(async () => {
    await rm(dir, { recursive: true, force: true });
  });

  it(
    "holds no events between a stream whose loop was left and the next watch()",
    async () => {
      const db = itemsDb(dir);
      await db.ready;
      try {
        const first = db.watch();
        await writeAtomically(dir, "first");
        for await (const event of first) {
          if (event.action === "insert") {
            break;
          }
        }
        await writeAtomically(dir, "unwatched");
        await indexed(db, "unwatched");

        const stream = db.watch();
        await writeAtomically(dir, "second");
        expect(await firstInsert(stream)).toBe("second");
        await stream.return();
      } finally {
        db.close();
      }
    },
    TEST_TIMEOUT,
  );

  it(
    "holds no events between a collected stream and the next watch()",
    async () => {
      const db = itemsDb(dir);
      await db.ready;
      try {
        let dropped: ReturnType<DirSQL["watch"]> | undefined = db.watch();
        await writeAtomically(dir, "first");
        expect(await firstInsert(dropped)).toBe("first");

        dropped = undefined;
        for (let round = 0; round < 3; round += 1) {
          collectGarbage();
          await sleep(20);
        }
        await writeAtomically(dir, "unwatched");
        await indexed(db, "unwatched");

        const stream = db.watch();
        await writeAtomically(dir, "second");
        expect(await firstInsert(stream)).toBe("second");
        await stream.return();
      } finally {
        db.close();
      }
    },
    TEST_TIMEOUT,
  );

  it(
    "ends a next() waiting for an event",
    async () => {
      const db = itemsDb(dir);
      await db.ready;
      try {
        const stream = db.watch();
        const waiting = stream.next();
        await sleep(50);

        const closing = stream.return();

        const outcome = await Promise.race([
          waiting,
          sleep(1000).then(() => "still waiting"),
        ]);
        expect(outcome).toEqual({ value: undefined, done: true });
        await closing;
      } finally {
        db.close();
      }
    },
    TEST_TIMEOUT,
  );

  it(
    "keeps an open stream live when another closes",
    async () => {
      const db = itemsDb(dir);
      await db.ready;
      try {
        const kept = db.watch();
        const closing = db.watch();
        await writeAtomically(dir, "first");
        expect(await firstInsert(closing)).toBe("first");
        await closing.return();

        await writeAtomically(dir, "second");
        expect(await firstInsert(kept)).toBe("first");
        expect(await firstInsert(kept)).toBe("second");
        await kept.return();
      } finally {
        db.close();
      }
    },
    TEST_TIMEOUT,
  );
});
