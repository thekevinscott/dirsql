// Extract a cargo-dist archive. Shells out to system `tar` for xz (Node
// stdlib lacks lzma) and `unzip` for zip.

import { spawnSync } from "node:child_process";
import { mkdirSync } from "node:fs";

export function extract(
  archive: string,
  dest: string,
  ext: "tar.xz" | "zip",
): void {
  mkdirSync(dest, { recursive: true });
  const result =
    ext === "tar.xz"
      ? spawnSync("tar", ["-xJf", archive, "-C", dest], { stdio: "inherit" })
      : spawnSync("unzip", ["-q", "-o", archive, "-d", dest], {
          stdio: "inherit",
        });
  if (result.status !== 0) {
    throw new Error(`extraction failed (exit ${result.status}) for ${archive}`);
  }
}
