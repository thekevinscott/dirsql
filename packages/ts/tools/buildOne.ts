// Build the per-platform sub-package for a single target: unpack the
// cargo-dist archive, move the binary into place, and emit a minimal
// package.json + README. Returns the emitted package directory.

import { createHash } from "node:crypto";
import {
  existsSync,
  mkdirSync,
  readFileSync,
  renameSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { join } from "node:path";
import type { Platform } from "../ts/platforms.js";
import { extract } from "./extract.js";
import { findBinary } from "./findBinary.js";

export function buildOne(
  p: Platform,
  distDir: string,
  outDir: string,
  version: string,
): string {
  const archive = join(distDir, `dirsql-${p.triple}.${p.ext}`);
  if (!existsSync(archive)) {
    throw new Error(
      `missing archive: ${archive} (run \`dist build\` or download the release)`,
    );
  }

  const pkgDir = join(outDir, p.name.replace("/", "__"));
  mkdirSync(pkgDir, { recursive: true });

  const extractRoot = join(pkgDir, ".extract");
  extract(archive, extractRoot, p.ext);

  const binName = p.exe ? "dirsql.exe" : "dirsql";
  const binSrc = findBinary(extractRoot, binName);
  if (!binSrc) {
    throw new Error(`dirsql binary not found inside ${archive}`);
  }
  const binDest = join(pkgDir, binName);
  renameSync(binSrc, binDest);
  rmSync(extractRoot, { recursive: true, force: true });

  const pkgJson: Record<string, unknown> = {
    name: p.name,
    version,
    description: `Platform-specific \`dirsql\` CLI binary for ${p.os[0]}-${p.cpu[0]}.`,
    license: "MIT",
    repository: "https://github.com/thekevinscott/dirsql",
    os: p.os,
    cpu: p.cpu,
    bin: { dirsql: `./${binName}` },
    files: [binName, "README.md"],
  };
  if (p.libc) pkgJson.libc = p.libc;
  writeFileSync(
    join(pkgDir, "package.json"),
    `${JSON.stringify(pkgJson, null, 2)}\n`,
  );
  writeFileSync(
    join(pkgDir, "README.md"),
    `# ${p.name}\n\nDo not install directly. Installed as an optional dependency of [\`dirsql\`](https://www.npmjs.com/package/dirsql) for users on ${p.os[0]}-${p.cpu[0]}.\n`,
  );

  const sha = createHash("sha256").update(readFileSync(binDest)).digest("hex");
  process.stdout.write(
    `built ${p.name} v${version} (${binName}, sha256=${sha})\n`,
  );
  return pkgDir;
}
