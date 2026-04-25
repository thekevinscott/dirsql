// Replaces the `docs` symlink in this package with a real copy of the
// workspace docs (markdown only) before `npm pack`, so the published tarball
// contains the docs files and not just a dangling symlink. After packing,
// restores the symlink.
//
// Invoked by npm lifecycle scripts: `prepack` -> `pre`, `postpack` -> `post`.

import {
  cpSync,
  existsSync,
  lstatSync,
  readdirSync,
  rmSync,
  symlinkSync,
} from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

// Markdown files at the docs root that should ship with the package.
export const KEEP_FILES = new Set([
  "index.md",
  "getting-started.md",
  "migrations.md",
]);

// Subdirectories of docs whose contents (recursively) should ship.
export const KEEP_DIRS = new Set(["guide", "api"]);

export function stageDocs(workspaceDocs: string, packageDocs: string): void {
  if (existsSync(packageDocs) || lstatSafe(packageDocs)) {
    rmSync(packageDocs, { recursive: true, force: true });
  }

  for (const entry of readdirSync(workspaceDocs, { withFileTypes: true })) {
    if (entry.isDirectory() && KEEP_DIRS.has(entry.name)) {
      cpSync(
        join(workspaceDocs, entry.name),
        join(packageDocs, entry.name),
        { recursive: true },
      );
    } else if (entry.isFile() && KEEP_FILES.has(entry.name)) {
      cpSync(join(workspaceDocs, entry.name), join(packageDocs, entry.name));
    }
  }
}

export function restoreSymlink(packageDocs: string, target: string): void {
  if (existsSync(packageDocs) || lstatSafe(packageDocs)) {
    rmSync(packageDocs, { recursive: true, force: true });
  }
  symlinkSync(target, packageDocs);
}

// `existsSync` returns false for broken symlinks; `lstatSync` throws. This
// helper returns the lstat result (truthy) for any path that has an entry,
// including broken symlinks, and `null` otherwise.
function lstatSafe(path: string): ReturnType<typeof lstatSync> | null {
  try {
    return lstatSync(path);
  } catch {
    return null;
  }
}

// CLI entry point — executed via the npm `prepack` / `postpack` hooks.
// `import.meta.url` is undefined when this module is imported by tests, so the
// CLI block is skipped during test runs.
const isCli =
  typeof import.meta.url === "string" &&
  process.argv[1] === fileURLToPath(import.meta.url);

if (isCli) {
  const phase = process.argv[2];
  const here = dirname(fileURLToPath(import.meta.url));
  const packageDir = resolve(here, "..");
  const packageDocs = join(packageDir, "docs");
  const workspaceDocs = resolve(packageDir, "..", "..", "docs");
  const symlinkTarget = "../../docs";

  if (phase === "pre") {
    stageDocs(workspaceDocs, packageDocs);
    console.log(`stageDocs(pre) -> staged workspace docs into ${packageDocs}`);
  } else if (phase === "post") {
    restoreSymlink(packageDocs, symlinkTarget);
    console.log(`stageDocs(post) -> restored symlink ${packageDocs} -> ${symlinkTarget}`);
  } else {
    console.error(
      `stageDocs: unknown phase '${phase}' (expected 'pre' or 'post')`,
    );
    process.exit(1);
  }
}
