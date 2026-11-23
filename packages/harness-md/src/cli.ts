#!/usr/bin/env node
import { spawnSync } from 'node:child_process';
import { glob, stat } from 'node:fs/promises';
import path from 'node:path';
import { parseArgs } from 'node:util';
import type { Language } from './assign.js';
import { write } from './write.js';

const { values, positionals } = parseArgs({
  allowPositionals: true,
  options: {
    input: { type: 'string' },
    language: { type: 'string' },
    out: { type: 'string' },
  },
});

if (values.input === undefined) {
  console.error('usage: harness-md --input <file or folder> [--language <language>] [--out <path pattern>] ["command"]');
  process.exit(2);
}

const files: string[] = [];
if ((await stat(values.input)).isDirectory()) {
  for await (const file of glob('**/*.md', { cwd: values.input })) {
    files.push(path.join(values.input, file));
  }
  files.sort();
} else {
  files.push(values.input);
}

for (const file of files) {
  for (const written of await write(file, { language: values.language as Language | undefined, out: values.out })) {
    console.error(path.relative(process.cwd(), written));
  }
}

if (positionals.length > 0) {
  process.exit(spawnSync(positionals.join(' '), { shell: true, stdio: 'inherit' }).status ?? 1);
}
