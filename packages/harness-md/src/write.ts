import { mkdir, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { assign, type Language } from './assign.js';
import { readConfig } from './config.js';
import { readDocument } from './document.js';
import { evaluate } from './evaluate.js';

export interface WriteOptions {
  root?: string;
  /** Write only this language. */
  language?: Language;
  /** Path pattern for `language`, in place of the one in harnessmd.toml. */
  out?: string;
}

interface Leaf {
  path: string[];
  fileLanguage: Language;
  cli?: string[];
}

const within = (outer: string[], inner: string[]) => outer.every((heading, depth) => inner[depth] === heading);

export const write = async (file: string, { root = process.cwd(), language, out: outOverride }: WriteOptions = {}): Promise<string[]> => {
  const { headings, out, cli = {} } = await readConfig(root, file);
  const items = readDocument(await readFile(file, 'utf8'));
  const languageDepth = headings.indexOf('language');
  const surfaceDepth = headings.indexOf('surface');

  const leaves = new Map<string, Leaf>();
  for (const item of items) {
    if (item.type !== 'code' || item.language === undefined) {
      continue;
    }
    if (item.path.length === headings.length && item.path[languageDepth] === item.language) {
      leaves.set(item.path.join('/'), {
        path: item.path,
        fileLanguage: item.language as Language,
        cli: item.path[surfaceDepth] === 'cli'
          ? cli[item.language]?.map(part => part.replaceAll('{root}', path.resolve(root)))
          : undefined,
      });
    }
  }

  const written: string[] = [];
  for (const leaf of leaves.values()) {
    const pattern = outOverride ?? out[leaf.fileLanguage];
    if (pattern === undefined || (language !== undefined && leaf.fileLanguage !== language)) {
      continue;
    }
    const pieces: string[] = leaf.cli ? [assign[leaf.fileLanguage]('cli', leaf.cli)] : [];
    const placed = new Map<string, number>();
    for (const item of items) {
      if (!within(item.path, leaf.path)) {
        continue;
      }
      if (item.type === 'data') {
        // Data under a nearer heading replaces data of the same name above it.
        const at = placed.get(item.name) ?? pieces.length;
        placed.set(item.name, at);
        pieces[at] = assign[leaf.fileLanguage](item.name, await evaluate(item));
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
