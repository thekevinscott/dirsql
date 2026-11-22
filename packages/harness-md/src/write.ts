import { mkdir, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { assign, type Language } from './assign.js';
import { readConfig } from './config.js';
import { readDocument } from './document.js';
import { evaluate } from './evaluate.js';

export interface WriteOptions {
  root?: string;
}

const within = (outer: string[], inner: string[]) => outer.every((heading, depth) => inner[depth] === heading);

export const write = async (file: string, { root = process.cwd() }: WriteOptions = {}): Promise<string[]> => {
  const { headings, out } = await readConfig(root);
  const items = readDocument(await readFile(file, 'utf8'));
  const languageDepth = headings.indexOf('language');

  const leaves = new Map<string, string[]>();
  for (const item of items) {
    if (item.type === 'code' && item.path.length === headings.length && item.path[languageDepth] === item.language) {
      leaves.set(item.path.join('/'), item.path);
    }
  }

  const written: string[] = [];
  for (const leaf of leaves.values()) {
    const language = leaf[languageDepth] as Language;
    const pattern = out[language];
    if (pattern === undefined) {
      continue;
    }
    const pieces: string[] = [];
    for (const item of items) {
      if (!within(item.path, leaf)) {
        continue;
      }
      if (item.type === 'data') {
        pieces.push(assign[language](item.name, await evaluate(item)));
      } else if (item.language === language) {
        pieces.push(item.content);
      }
    }
    const target = path.resolve(root, pattern.replaceAll(/\{(\w+)\}/g, (_, key: string) => leaf[headings.indexOf(key)] ?? `{${key}}`));
    await mkdir(path.dirname(target), { recursive: true });
    await writeFile(target, `${pieces.join('\n\n')}\n`);
    written.push(target);
  }
  return written;
};
