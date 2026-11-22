import { mkdir, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { assign } from './assign.js';
import { readDocument } from './document.js';
import { evaluate } from './evaluate.js';

export type Language = 'python' | 'typescript' | 'rust' | 'bash';

export interface WriteOptions {
  language: Language;
  outDir?: string;
}

const fileName: Record<Language, (stem: string) => string> = {
  python: stem => `${stem.replaceAll(/[ -]+/g, '_')}_test.py`,
  typescript: stem => `${stem.replaceAll(/[ _]+/g, '-')}.test.ts`,
  rust: stem => `${stem.replaceAll(/[ -]+/g, '_')}.rs`,
  bash: stem => `${stem.replaceAll(/[ _]+/g, '-')}.sh`,
};

const contains = (outer: number[], inner: number[]) => outer.every((heading, depth) => inner[depth] === heading);

export const write = async (file: string, { language, outDir = 'tests/build' }: WriteOptions): Promise<string> => {
  const items = readDocument(await readFile(file, 'utf8'));
  const code = items.filter(item => item.type === 'code' && item.language === language);

  const pieces: string[] = [];
  for (const item of items) {
    if (item.type === 'code') {
      if (item.language === language) {
        pieces.push(item.content);
      }
    } else if (code.some(block => contains(item.path, block.path))) {
      pieces.push(assign[language](item.name, await evaluate(item)));
    }
  }

  const dir = path.resolve(outDir);
  await mkdir(dir, { recursive: true });
  const target = path.join(dir, fileName[language](path.parse(file).name));
  await writeFile(target, `${pieces.join('\n\n')}\n`);
  return target;
};
