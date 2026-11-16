import { mkdir, mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DirSQL } from "dirsql";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

// inotify refuses to watch an unreadable directory, except for root.
const unwatchable = process.platform === "linux" && process.getuid?.() !== 0;

describe.runIf(unwatchable)("a directory the watcher cannot watch", () => {
  let dir: string;

  beforeEach(async () => {
    dir = await mkdtemp(join(tmpdir(), "dirsql-watch-fault-"));
  });

  afterEach(async () => {
    await rm(dir, { recursive: true, force: true });
  });

  it("fails every query after it appears", async () => {
    const db = new DirSQL({
      root: dir,
      tables: [
        {
          name: "items",
          ddl: "CREATE TABLE items (name TEXT)",
          glob: "**/*.md",
          onFile: (filePath: string) => [{ name: filePath }],
        },
      ],
    });
    await db.ready;
    expect(await db.query("SELECT name FROM items")).toEqual([]);

    await mkdir(join(dir, "locked"), { mode: 0o000 });

    for (let i = 0; i < 2; i++) {
      await expect(db.query("SELECT name FROM items")).rejects.toThrow(
        /Permission denied/,
      );
    }
  });
});
