import { readFile } from 'node:fs/promises';
import path from 'node:path';
import deepmerge from 'deepmerge';
import { parse } from 'smol-toml';
import type { Language } from './assign.js';

export interface Config {
  headings: string[];
  out: Partial<Record<Language, string>>;
  cli?: Record<string, string[]>;
}

const defaults: Config = {
  headings: ['test', 'surface', 'language'],
  out: {
    python: '.harnessmd/{language}/{surface}/{test}_test.py',
    typescript: '.harnessmd/{language}/{surface}/{test}.test.ts',
    rust: '.harnessmd/{language}/{surface}/{test}.rs',
  },
};

const readToml = async (folder: string): Promise<Partial<Config>> => {
  try {
    return parse(await readFile(path.join(folder, 'harnessmd.toml'), 'utf8')) as Partial<Config>;
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === 'ENOENT') {
      return {};
    }
    throw error;
  }
};

/** Every folder from `root` down to the folder holding `file`, outermost first. */
const foldersBetween = (root: string, file: string): string[] => {
  const steps = path.relative(root, path.dirname(path.resolve(file))).split(path.sep);
  if (steps[0] === '..' || steps[0] === '') {
    return [root];
  }
  return steps.reduce((folders, step) => [...folders, path.join(folders.at(-1)!, step)], [root]);
};

/** harnessmd.toml files nearer the markdown file win, key by key; a list replaces the list above it. */
export const readConfig = async (root: string, file: string): Promise<Config> => {
  const layers = await Promise.all(foldersBetween(path.resolve(root), file).map(readToml));
  return deepmerge.all<Config>([defaults, ...layers], { arrayMerge: (_above, nearer) => nearer });
};
