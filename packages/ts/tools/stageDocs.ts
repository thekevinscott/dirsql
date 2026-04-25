// Replaces the `docs` symlink with a real copy of the workspace docs (markdown
// only) before `npm pack`, so the published tarball contains the docs files
// and not just a dangling symlink. After packing, restores the symlink.
//
// Invoked by `npm` lifecycle scripts: `prepack` -> `pre`, `postpack` -> `post`.

import { execSync } from 'node:child_process';
import {
  cpSync,
  existsSync,
  lstatSync,
  readdirSync,
  rmSync,
  symlinkSync,
} from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const phase = process.argv[2];
const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const PACKAGE_DIR = resolve(__dirname, '..');
const DOCS_LINK = join(PACKAGE_DIR, 'docs');
const WORKSPACE_DOCS = resolve(PACKAGE_DIR, '..', '..', 'docs');

// Patterns to keep when staging the docs into the published package. Anything
// else under docs/ (VitePress build tooling, agent meta, tests) stays out.
const KEEP = new Set(['index.md', 'getting-started.md', 'migrations.md']);
const KEEP_DIRS = new Set(['guide', 'api']);

function stageDocs(): void {
  if (existsSync(DOCS_LINK) && lstatSync(DOCS_LINK).isSymbolicLink()) {
    rmSync(DOCS_LINK);
  } else if (existsSync(DOCS_LINK)) {
    rmSync(DOCS_LINK, { recursive: true, force: true });
  }

  for (const entry of readdirSync(WORKSPACE_DOCS, { withFileTypes: true })) {
    if (entry.isDirectory() && KEEP_DIRS.has(entry.name)) {
      cpSync(
        join(WORKSPACE_DOCS, entry.name),
        join(DOCS_LINK, entry.name),
        { recursive: true },
      );
    } else if (entry.isFile() && KEEP.has(entry.name)) {
      cpSync(join(WORKSPACE_DOCS, entry.name), join(DOCS_LINK, entry.name));
    }
  }
}

function restoreSymlink(): void {
  if (existsSync(DOCS_LINK)) {
    rmSync(DOCS_LINK, { recursive: true, force: true });
  }
  symlinkSync('../../docs', DOCS_LINK);
}

if (phase === 'pre') {
  stageDocs();
} else if (phase === 'post') {
  restoreSymlink();
} else {
  console.error(`stageDocs: unknown phase '${phase}' (expected 'pre' or 'post')`);
  process.exit(1);
}

// Light log so CI shows what happened.
const entries = existsSync(DOCS_LINK)
  ? execSync(`find ${DOCS_LINK} -maxdepth 2 -type f`, { encoding: 'utf8' })
  : '';
console.log(`stageDocs(${phase}) -> docs/ now contains:\n${entries}`);
