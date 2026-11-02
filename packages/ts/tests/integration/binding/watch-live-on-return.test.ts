import { readFileSync } from "node:fs";
import { mkdtemp, rename, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DirSQL, type RowEvent } from "dirsql";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

const TEST_TIMEOUT = 15_000;

function itemsDb(root: string): DirSQL {
  return new DirSQL({
    root,
    tables: [
      {
        name: "items",
        ddl: "CREATE TABLE items (name TEXT)",
        glob: "**/*.json",
        onFile: (filePath: string) => [
          JSON.parse(readFileSync(filePath, "utf8")),
        ],
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
  stream: AsyncIterable<RowEvent>,
): Promise<string | undefined> {
  for await (const event of stream) {
    if (event.action === "insert") {
      return String(event.row?.name);
    }
  }
  return undefined;
}

function within<T>(promise: Promise<T>, ms: number): Promise<T | "timeout"> {
  return Promise.race([
    promise,
    new Promise<"timeout">((resolve) => setTimeout(resolve, ms, "timeout")),
  ]);
}

describe("watch() is live on return", () => {
  let dir: string;

  beforeEach(async () => {
    dir = await mkdtemp(join(tmpdir(), "dirsql-watch-live-"));
  });

  afterEach(async () => {
    await rm(dir, { recursive: true, force: true });
  });

  it(
    "sees a write made right after watch() returns",
    async () => {
      const db = itemsDb(dir);
      await db.ready;
      try {
        const stream = db.watch();
        await writeAtomically(dir, "advice");

        expect(await within(firstInsert(stream), 5000)).toBe("advice");
        expect(await db.query("SELECT name FROM items")).toEqual([
          { name: "advice" },
        ]);
      } finally {
        db.close();
      }
    },
    TEST_TIMEOUT,
  );

  it(
    "sees a write made after watch() then ready",
    async () => {
      const db = itemsDb(dir);
      try {
        const stream = db.watch();
        await db.ready;
        await writeAtomically(dir, "advice");

        expect(await within(firstInsert(stream), 5000)).toBe("advice");
        expect(await db.query("SELECT name FROM items")).toEqual([
          { name: "advice" },
        ]);
      } finally {
        db.close();
      }
    },
    TEST_TIMEOUT,
  );
});
