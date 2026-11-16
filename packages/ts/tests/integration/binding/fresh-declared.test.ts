import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  realpathSync,
  renameSync,
  rmSync,
  statSync,
  unlinkSync,
  utimesSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, sep } from "node:path";
import { isDeepStrictEqual } from "node:util";
import { Worker } from "node:worker_threads";
import { DirSQL, type RowEvent } from "dirsql";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

const ROUNDS = 2000;
const TEST_TIMEOUT = 120_000;

function notesDb(root: string): DirSQL {
  const bases = [realpathSync(root), root];
  return new DirSQL({
    root,
    tables: [
      {
        name: "notes",
        ddl: "CREATE TABLE notes (file TEXT, line TEXT)",
        glob: "**/*.txt",
        onFile: (filePath: string) => {
          let rel = filePath;
          for (const base of bases) {
            if (filePath.startsWith(base + sep)) {
              rel = filePath.slice(base.length + 1);
              break;
            }
          }
          rel = rel.split(sep).join("/");
          return readFileSync(filePath, "utf8")
            .split("\n")
            .filter((line) => line.length > 0)
            .map((line) => ({ file: rel, line }));
        },
      },
    ],
  });
}

async function open(root: string): Promise<DirSQL> {
  const db = notesDb(root);
  await db.ready;
  return db;
}

function write(root: string, rel: string, content: string): void {
  const path = join(root, rel);
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, content);
}

function saveAtomically(root: string, rel: string, content: string): void {
  write(root, `${rel}.stage`, content);
  renameSync(join(root, `${rel}.stage`), join(root, rel));
}

function times(root: string, rel: string): [Date, Date] {
  const st = statSync(join(root, rel));
  return [st.atime, st.mtime];
}

function restoreTimes(root: string, rel: string, [atime, mtime]: [Date, Date]) {
  utimesSync(join(root, rel), atime, mtime);
}

async function lines(db: DirSQL, rel: string): Promise<string[]> {
  const rows = await db.query(
    `SELECT line FROM notes WHERE file = '${rel}' ORDER BY line`,
  );
  return rows.map((row) => String(row.line));
}

async function awaitEq<T>(
  read: () => Promise<T>,
  expected: T,
  context: string,
): Promise<void> {
  let seen = await read();
  for (let i = 0; i < 1000 && !isDeepStrictEqual(seen, expected); i++) {
    await new Promise((resolve) => setTimeout(resolve, 10));
    seen = await read();
  }
  expect(seen, context).toEqual(expected);
}

async function awaitLines(
  db: DirSQL,
  rel: string,
  expected: string[],
  context = rel,
): Promise<void> {
  await awaitEq(() => lines(db, rel), expected, context);
}

async function snapshot(db: DirSQL): Promise<[string, string][]> {
  const rows = await db.query(
    "SELECT file, line FROM notes ORDER BY file, line",
  );
  return rows.map((row) => [String(row.file), String(row.line)]);
}

async function awaitFreshScan(
  db: DirSQL,
  root: string,
  context: string,
): Promise<void> {
  const fresh = await open(root);
  try {
    const expected = await snapshot(fresh);
    await awaitEq(() => snapshot(db), expected, context);
  } finally {
    fresh.close();
  }
}

function rng(seed: number): (n: number) => number {
  let state = seed >>> 0;
  return (n) => {
    state = (state + 0x6d2b79f5) >>> 0;
    let t = state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return (((t ^ (t >>> 14)) >>> 0) / 4294967296) * n;
  };
}

const DIRS = ["", "a/", "a/b/", "c/"];
const NAMES = ["n0", "n1", "n2", "n3"];
const EXTS = ["txt", "json", "stage"];

function pick<T>(below: (n: number) => number, items: T[]): T {
  return items[Math.floor(below(items.length))];
}

function content(below: (n: number) => number): string {
  let text = "";
  const count = 1 + Math.floor(below(3));
  for (let i = 0; i < count; i++) {
    text += `line-${String(Math.floor(below(10_000))).padStart(4, "0")}\n`;
  }
  return text;
}

function randomFile(below: (n: number) => number): string {
  return `${pick(below, DIRS)}${pick(below, NAMES)}.${pick(below, EXTS)}`;
}

function existing(root: string): string[] {
  const files: string[] = [];
  for (const d of DIRS) {
    for (const n of NAMES) {
      for (const e of EXTS) {
        const rel = `${d}${n}.${e}`;
        if (existsSync(join(root, rel)) && statSync(join(root, rel)).isFile()) {
          files.push(rel);
        }
      }
    }
  }
  return files;
}

function step(root: string, below: (n: number) => number): string {
  const files = existing(root);
  const op = Math.floor(below(9));
  if (op === 1 && files.length > 0) {
    const rel = pick(below, files);
    const before = times(root, rel);
    const flipped = readFileSync(join(root, rel), "utf8").replace(/\d/g, (d) =>
      String((Number(d) + 1) % 10),
    );
    write(root, rel, flipped);
    restoreTimes(root, rel, before);
    return `same-size edit of ${rel} with its times restored`;
  }
  if (op === 2) {
    const rel = randomFile(below);
    saveAtomically(root, rel, content(below));
    return `atomic save of ${rel}`;
  }
  if (op === 3 && files.length > 0) {
    const rel = pick(below, files);
    unlinkSync(join(root, rel));
    return `delete ${rel}`;
  }
  if (op === 4 && files.length > 0) {
    const from = pick(below, files);
    const to = randomFile(below);
    if (from === to) {
      return "nothing";
    }
    mkdirSync(dirname(join(root, to)), { recursive: true });
    renameSync(join(root, from), join(root, to));
    return `rename ${from} to ${to}`;
  }
  if (op === 5) {
    const dir = `${pick(below, DIRS)}new${Math.floor(below(3))}`;
    for (const name of NAMES.slice(0, 2)) {
      write(root, `${dir}/${name}.${pick(below, EXTS)}`, content(below));
    }
    return `populate new directory ${dir}`;
  }
  if (op === 6) {
    const dir = pick(below, DIRS.slice(1)).replace(/\/$/, "");
    rmSync(join(root, dir), { recursive: true, force: true });
    return `remove directory ${dir}`;
  }
  const rel = randomFile(below);
  write(root, rel, content(below));
  return `write ${rel}`;
}

function seedTree(root: string, below: (n: number) => number): void {
  for (let i = 0; i < 8; i++) {
    write(root, randomFile(below), content(below));
  }
}

describe.runIf(process.platform === "linux")(
  "every write reaches a declared table's index",
  () => {
    let dir: string;
    let db: DirSQL | undefined;

    beforeEach(() => {
      dir = mkdtempSync(join(tmpdir(), "dirsql-fresh-"));
      db = undefined;
    });

    afterEach(() => {
      db?.close();
      rmSync(dir, { recursive: true, force: true });
    });

    describe("one write at a time", () => {
      it(
        "every create reaches the index",
        async () => {
          db = await open(dir);
          for (let i = 0; i < ROUNDS; i++) {
            write(dir, `h/${i}.txt`, `c${i}`);
            await awaitLines(db, `h/${i}.txt`, [`c${i}`], `round ${i}`);
            unlinkSync(join(dir, `h/${i}.txt`));
          }
        },
        TEST_TIMEOUT,
      );

      it(
        "every edit reaches the index",
        async () => {
          write(dir, "e.txt", "start");
          db = await open(dir);
          for (let i = 0; i < ROUNDS; i++) {
            write(dir, "e.txt", `e${i}`);
            await awaitLines(db, "e.txt", [`e${i}`], `round ${i}`);
          }
        },
        TEST_TIMEOUT,
      );

      it(
        "every delete reaches the index",
        async () => {
          db = await open(dir);
          for (let i = 0; i < ROUNDS; i++) {
            write(dir, "d.txt", `d${i}`);
            await awaitLines(db, "d.txt", [`d${i}`], `round ${i}`);
            unlinkSync(join(dir, "d.txt"));
            await awaitLines(db, "d.txt", [], `round ${i}`);
          }
        },
        TEST_TIMEOUT,
      );

      it(
        "every rename into and out of the glob reaches the index",
        async () => {
          db = await open(dir);
          for (let i = 0; i < ROUNDS; i++) {
            write(dir, "r.stage", `r${i}`);
            renameSync(join(dir, "r.stage"), join(dir, "r.txt"));
            await awaitLines(db, "r.txt", [`r${i}`], `round ${i}`);
            renameSync(join(dir, "r.txt"), join(dir, "r.stage"));
            await awaitLines(db, "r.txt", [], `round ${i}`);
          }
        },
        TEST_TIMEOUT,
      );

      it(
        "every atomic save reaches the index",
        async () => {
          write(dir, "a.txt", "start");
          db = await open(dir);
          for (let i = 0; i < ROUNDS; i++) {
            saveAtomically(dir, "a.txt", `a${i}`);
            await awaitLines(db, "a.txt", [`a${i}`], `round ${i}`);
          }
        },
        TEST_TIMEOUT,
      );

      it(
        "every same-size edit reaches the index",
        async () => {
          write(dir, "s.txt", "s------");
          db = await open(dir);
          for (let i = 0; i < ROUNDS; i++) {
            const text = `s${String(i).padStart(6, "0")}`;
            write(dir, "s.txt", text);
            await awaitLines(db, "s.txt", [text], `round ${i}`);
          }
        },
        TEST_TIMEOUT,
      );

      it(
        "every same-size edit with its times restored reaches the index",
        async () => {
          write(dir, "t.txt", "t------");
          const before = times(dir, "t.txt");
          db = await open(dir);
          for (let i = 0; i < ROUNDS; i++) {
            const text = `t${String(i).padStart(6, "0")}`;
            write(dir, "t.txt", text);
            restoreTimes(dir, "t.txt", before);
            await awaitLines(db, "t.txt", [text], `round ${i}`);
          }
        },
        TEST_TIMEOUT,
      );
    });

    describe("a long-lived instance", () => {
      it(
        "reads what a fresh scan reads after random operations",
        async () => {
          for (let seed = 1; seed <= 3; seed++) {
            const root = join(dir, `seed${seed}`);
            mkdirSync(root);
            const below = rng(seed);
            seedTree(root, below);
            db?.close();
            db = await open(root);
            for (let op = 0; op < 100; op++) {
              const done = step(root, below);
              await awaitFreshScan(db, root, `seed ${seed} op ${op}: ${done}`);
            }
          }
        },
        TEST_TIMEOUT,
      );

      it(
        "reads what a fresh scan reads across idle gaps and bursts",
        async () => {
          const below = rng(99);
          seedTree(dir, below);
          db = await open(dir);
          for (let cycle = 0; cycle < 10; cycle++) {
            await new Promise((resolve) =>
              setTimeout(resolve, 250 * (cycle % 4)),
            );
            for (let op = 0; op < 20; op++) {
              const done = step(dir, below);
              await awaitFreshScan(db, dir, `cycle ${cycle} op ${op}: ${done}`);
            }
          }
        },
        TEST_TIMEOUT,
      );
    });

    describe("each way a change could be missed", () => {
      it("reads an edit whose times are put back", async () => {
        write(dir, "touched.txt", "before");
        const before = times(dir, "touched.txt");
        db = await open(dir);
        await awaitLines(db, "touched.txt", ["before"]);
        write(dir, "touched.txt", "after!");
        restoreTimes(dir, "touched.txt", before);
        await awaitLines(db, "touched.txt", ["after!"]);
      });

      it("reads a file deleted and recreated under the same name", async () => {
        write(dir, "again.txt", "first");
        db = await open(dir);
        await awaitLines(db, "again.txt", ["first"]);
        unlinkSync(join(dir, "again.txt"));
        write(dir, "again.txt", "secnd");
        await awaitLines(db, "again.txt", ["secnd"]);
      });

      it("reads a new and a deleted subdirectory", async () => {
        db = await open(dir);
        write(dir, "sub/deep/one.txt", "one");
        await awaitLines(db, "sub/deep/one.txt", ["one"]);
        rmSync(join(dir, "sub"), { recursive: true });
        await awaitLines(db, "sub/deep/one.txt", []);
      });

      it(
        "never reads a file torn while it is rewritten, and ends exact",
        async () => {
          const version = (k: number) =>
            `v${String(k).padStart(6, "0")}\n`.repeat(50);
          write(dir, "busy.txt", version(0));
          db = await open(dir);
          const writer = new Worker(
            `const { renameSync, writeFileSync } = require("node:fs");
          const { join } = require("node:path");
          const { workerData } = require("node:worker_threads");
          for (let k = 1; k <= 300; k++) {
            const text = ("v" + String(k).padStart(6, "0") + "\\n").repeat(50);
            writeFileSync(join(workerData, "busy.txt.stage"), text);
            renameSync(join(workerData, "busy.txt.stage"), join(workerData, "busy.txt"));
          }`,
            { eval: true, workerData: dir },
          );
          let finished = false;
          const exited = new Promise<void>((resolve, reject) => {
            writer.on("error", reject);
            writer.on("exit", () => {
              finished = true;
              resolve();
            });
          });
          while (!finished) {
            const seen = await lines(db, "busy.txt");
            expect(seen.length, "a partial version").toBe(50);
            expect(new Set(seen).size, "mixed versions").toBe(1);
          }
          await exited;
          await awaitLines(
            db,
            "busy.txt",
            version(300)
              .split("\n")
              .filter((line) => line.length > 0),
          );
        },
        TEST_TIMEOUT,
      );
    });

    describe("watch consumers", () => {
      it(
        "a watch stream gets each change once",
        async () => {
          db = await open(dir);
          const stream = db.watch()[Symbol.asyncIterator]();
          const files = 100;
          for (let i = 0; i < files; i++) {
            write(dir, `w/${i}.txt`, `w${i}`);
            await awaitLines(db, `w/${i}.txt`, [`w${i}`], `file ${i}`);
          }
          // Written last, so the stream reports it after every change before
          // it.
          write(dir, "sentinel.txt", "end");
          const counts = new Map<string, number>();
          for (;;) {
            const next = await stream.next();
            expect(next.done).toBe(false);
            const event: RowEvent = next.value;
            expect(event.action).toBe("insert");
            const line = String(event.row?.line);
            if (line === "end") {
              break;
            }
            counts.set(line, (counts.get(line) ?? 0) + 1);
          }
          expect(Object.fromEntries(counts)).toEqual(
            Object.fromEntries(
              Array.from({ length: files }, (_, i) => [`w${i}`, 1]),
            ),
          );
        },
        TEST_TIMEOUT,
      );
    });
  },
);
