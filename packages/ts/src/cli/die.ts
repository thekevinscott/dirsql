// Exit with a `dirsql: ` prefixed error message.

export function die(msg: string, code = 1): never {
  process.stderr.write(`dirsql: ${msg}\n`);
  process.exit(code);
}
