import { readdirSync, readFileSync, readlinkSync } from "node:fs";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { basename, join } from "node:path";
import { DirSQL } from "dirsql";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

function inotifyWatches(): number {
  let count = 0;
  for (const fd of readdirSync("/proc/self/fd")) {
    try {
      if (readlinkSync(`/proc/self/fd/${fd}`) !== "anon_inode:inotify") {
        continue;
      }
      count += readFileSync(`/proc/self/fdinfo/${fd}`, "utf8")
        .split("\n")
        .filter((line) => line.startsWith("inotify wd:")).length;
    } catch {}
  }
  return count;
}

function itemsDb(root: string): DirSQL {
  return new DirSQL({
    root,
    watch: false,
    tables: [
      {
        name: "items",
        ddl: "CREATE TABLE items (name TEXT)",
        glob: "**/*.md",
        onFile: (filePath: string) => [{ name: basename(filePath) }],
      },
    ],
  });
}

describe("watch: false", () => {
  let dir: string;

  beforeEach(async () => {
    dir = await mkdtemp(join(tmpdir(), "dirsql-watch-off-"));
  });

  afterEach(async () => {
    await rm(dir, { recursive: true, force: true });
  });

  it.runIf(process.platform === "linux")(
    "reads the build walk without an inotify watch",
    async () => {
      await mkdir(join(dir, "sub"));
      await writeFile(join(dir, "sub", "built.md"), "");
      const before = inotifyWatches();

      const db = itemsDb(dir);
      await db.ready;
      const rows = await db.query("SELECT name FROM items");

      expect(rows).toEqual([{ name: "built.md" }]);
      expect(inotifyWatches() - before).toBe(0);
    },
  );

  it("throws on watch, startWatcher and pollEvents", async () => {
    const db = itemsDb(dir);
    expect(() => db.watch()).toThrow(/watching is off/);
    await db.ready;
    expect(() => db.watch()).toThrow(/watching is off/);
    await expect(db.startWatcher()).rejects.toThrow(/watching is off/);
    await expect(db.pollEvents(10)).rejects.toThrow(/watching is off/);
  });
});
