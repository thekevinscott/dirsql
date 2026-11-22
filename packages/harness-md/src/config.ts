import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { parse } from 'smol-toml';
import type { Language } from './assign.js';

export interface Config {
  headings: string[];
  out: Partial<Record<Language, string>>;
}

export const readConfig = async (root: string): Promise<Config> =>
  parse(await readFile(path.join(root, 'harnessmd.toml'), 'utf8')) as unknown as Config;
