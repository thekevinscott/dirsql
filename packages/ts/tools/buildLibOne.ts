// Build the per-platform napi-rs sub-package for a single target: copy
// the prebuilt `.node` binary into place and emit a minimal package.json
// + README. Returns the emitted package directory.
//
// The input is `<nodeDir>/dirsql.<slug>.node` (what the CI matrix job
// produces via `napi build --target <triple>`). The output mirrors the
// layout `buildOne.ts` uses for CLI sub-packages but ships a `main`
// pointing at the `.node` binary so `require('@dirsql/lib-<slug>')`
// returns the native addon directly.

import { createHash } from "node:crypto";
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  writeFileSync,
} from "node:fs";
import { join } from "node:path";
import { type Platform, librarySlug } from "../ts/platforms.js";

export function buildLibOne(
  p: Platform,
  nodeDir: string,
  outDir: string,
  version: string,
): string {
  const slug = librarySlug(p);
  const binName = `dirsql.${slug}.node`;
  const binSrc = join(nodeDir, binName);
  if (!existsSync(binSrc)) {
    throw new Error(
      `missing napi artifact: ${binSrc} (run \`napi build --target ${p.triple}\` or download the CI artifact)`,
    );
  }

  const pkgDir = join(outDir, p.libName.replace("/", "__"));
  mkdirSync(pkgDir, { recursive: true });
  const binDest = join(pkgDir, binName);
  copyFileSync(binSrc, binDest);

  const pkgJson: Record<string, unknown> = {
    name: p.libName,
    version,
    description: `Platform-specific napi-rs binary for \`dirsql\` on ${p.os[0]}-${p.cpu[0]}.`,
    license: "MIT",
    repository: "https://github.com/thekevinscott/dirsql",
    main: binName,
    os: p.os,
    cpu: p.cpu,
    files: [binName, "README.md"],
  };
  if (p.libc) pkgJson.libc = p.libc;
  writeFileSync(
    join(pkgDir, "package.json"),
    `${JSON.stringify(pkgJson, null, 2)}\n`,
  );
  writeFileSync(
    join(pkgDir, "README.md"),
    `# ${p.libName}\n\nDo not install directly. Installed as an optional dependency of [\`dirsql\`](https://www.npmjs.com/package/dirsql) for users on ${p.os[0]}-${p.cpu[0]}.\n`,
  );

  const sha = createHash("sha256").update(readFileSync(binDest)).digest("hex");
  process.stdout.write(
    `built ${p.libName} v${version} (${binName}, sha256=${sha})\n`,
  );
  return pkgDir;
}
