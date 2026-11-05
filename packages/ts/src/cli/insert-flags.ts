const END_OF_OPTIONS = "--";

/**
 * Return `argv` with `flags` placed before the first `--`, or at the end when
 * there is none; after `--` the core reads them as positionals.
 */
export function insertFlags(argv: string[], flags: string[]): string[] {
  const end = argv.indexOf(END_OF_OPTIONS);
  if (end === -1) {
    return [...argv, ...flags];
  }
  return [...argv.slice(0, end), ...flags, ...argv.slice(end)];
}
