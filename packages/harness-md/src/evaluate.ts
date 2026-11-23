import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import type { Data } from './document.js';

const run = promisify(execFile);

const python = async (source: string): Promise<unknown> => {
  const { stdout } = await run('python3', ['-c', 'import json, sys; print(json.dumps(eval(sys.argv[1])))', source]);
  return JSON.parse(stdout);
};

export const evaluate = async ({ language, source }: Data): Promise<unknown> => {
  if (language === 'json') {
    return JSON.parse(source);
  }
  if (language === 'python') {
    return python(source);
  }
  return source;
};
