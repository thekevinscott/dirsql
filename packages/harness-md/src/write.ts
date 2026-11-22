import { mkdir, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { assign, type Language } from './assign.js';
import { readConfig } from './config.js';
import { readDocument } from './document.js';
import { evaluate } from './evaluate.js';

export interface WriteOptions {
  root?: string;
}

interface Leaf {
  path: string[];
  fileLanguage: Language;
  cli?: string[];
}

const within = (outer: string[], inner: string[]) => outer.every((heading, depth) => inner[depth] === heading);

export const write = async (file: string, { root = process.cwd() }: WriteOptions = {}): Promise<string[]> => {
  const { headings, out, cli = {} } = await readConfig(root);
  const items = readDocument(await readFile(file, 'utf8'));
  const languageDepth = headings.indexOf('language');
  const surfaceDepth = headings.indexOf('surface');

  const leaves = new Map<string, Leaf>();
  for (const item of items) {
    if (item.type !== 'code' || item.language === undefined) {
      continue;
    }
    if (item.path.length === headings.length && item.path[languageDepth] === item.language) {
      leaves.set(item.path.join('/'), { path: item.path, fileLanguage: item.language as Language });
    }
    // A block right under the cli heading is written once per launcher.
    if (item.path.length === languageDepth && item.path[surfaceDepth] === 'cli') {
      for (const [language, launcher] of Object.entries(cli)) {
        const leaf = [...item.path, language];
        leaves.set(leaf.join('/'), { path: leaf, fileLanguage: item.language as Language, cli: launcher });
      }
    }
  }

  const written: string[] = [];
  for (const leaf of leaves.values()) {
    const pattern = out[leaf.fileLanguage];
    if (pattern === undefined) {
      continue;
    }
    const pieces: string[] = leaf.cli ? [assign[leaf.fileLanguage]('cli', leaf.cli)] : [];
    for (const item of items) {
      if (!within(item.path, leaf.path)) {
        continue;
      }
      if (item.type === 'data') {
        pieces.push(assign[leaf.fileLanguage](item.name, await evaluate(item)));
      } else if (item.language === leaf.fileLanguage) {
        pieces.push(item.content);
      }
    }
    const target = path.resolve(root, pattern.replaceAll(/\{(\w+)\}/g, (_, key: string) => leaf.path[headings.indexOf(key)] ?? `{${key}}`));
    await mkdir(path.dirname(target), { recursive: true });
    await writeFile(target, `${pieces.join('\n\n')}\n`);
    written.push(target);
  }
  return written;
};
