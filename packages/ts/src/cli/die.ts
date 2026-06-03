// Exit with a `dirsql: ` prefixed error message. The `never` return type
// signals to callers that control doesn't return.

export function die(msg: string, code = 1): never {
  process.stderr.write(`dirsql: ${msg}\n`);
  process.exit(code);
}
