// The `-c`/`--config` flag is repeatable, so the scan collects every
// occurrence, in argv order.

/**
 * Every config value in argv, in order (`--config X`, `--config=X`, `-c X`,
 * `-c=X`, `-cX`), or the default when none are given.
 */
export function configPathsFromArgv(argv: string[]): string[] {
  const paths: string[] = [];
  let expectValue = false;
  for (const a of argv) {
    if (expectValue) {
      paths.push(a);
      expectValue = false;
    } else if (a === "--config" || a === "-c") {
      expectValue = true;
    } else if (a.startsWith("--config=")) {
      paths.push(a.slice("--config=".length));
    } else if (a.startsWith("-c")) {
      const value = a.slice("-c".length);
      paths.push(value.startsWith("=") ? value.slice("=".length) : value);
    }
  }
  if (expectValue) {
    // A bare trailing flag (no following value) yields "".
    paths.push("");
  }
  return paths.length > 0 ? paths : ["./.dirsql.toml"];
}
