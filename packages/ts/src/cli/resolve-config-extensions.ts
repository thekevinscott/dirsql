// Launcher-side resolution of the TOML configs' `[[dirsql.extension]]` entries.
//
// The compiled `dirsql` binary loads config extensions literally — it has no
// `require.resolve`, so it cannot resolve a bare **package name**. When any
// TOML config in argv names an extension by package name, the shared SDK
// resolver resolves every config's entries and this launcher passes the
// resolved literal paths to the binary via repeatable `--extension` flags;
// the binary then loads those and ignores the configs' own extension entries.
//
// The `-c`/`--config` flag is repeatable, so the scan collects every
// occurrence, in argv order.
//
// Native-language configs (`.py`/`.js`/`.mjs`/`.cjs`) are untouched: the binary
// dispatches those to `dirsql interpret`, whose handshake already carries
// resolved paths.

import { existsSync } from "node:fs";

// Config extensions the binary dispatches to `dirsql interpret`; never
// pre-resolved here (that path resolves via the handshake).
const NATIVE_CONFIG_SUFFIXES = [".py", ".js", ".mjs", ".cjs"];

/**
 * Every config value in argv, in order (`--config X`, `--config=X`, `-c X`,
 * `-c=X`, `-cX`), or the default when none are given.
 */
function configPathsFromArgv(argv: string[]): string[] {
  const paths: string[] = [];
  let i = 0;
  while (i < argv.length) {
    const a = argv[i];
    if (a === "--config" || a === "-c") {
      // A bare trailing flag (no following value) yields "".
      paths.push(argv[i + 1] ?? "");
      i += 2;
      continue;
    }
    if (a?.startsWith("--config=")) {
      paths.push(a.slice("--config=".length));
    } else if (a?.startsWith("-c")) {
      const value = a.slice("-c".length);
      paths.push(value.startsWith("=") ? value.slice("=".length) : value);
    }
    i++;
  }
  return paths.length > 0 ? paths : ["./.dirsql.toml"];
}

/**
 * Return `argv` augmented with `--extension <path>[::entrypoint]` flags when a
 * TOML config names an extension by package name; otherwise return `argv`
 * unchanged. Throws if a package name cannot be resolved (surfaced by the
 * launcher as a clean error).
 */
export async function withResolvedExtensions(
  argv: string[],
): Promise<string[]> {
  if (argv[0] === "init") {
    return argv;
  }
  const configPaths = configPathsFromArgv(argv).filter(
    (p) => !NATIVE_CONFIG_SUFFIXES.some((s) => p.endsWith(s)),
  );
  // The shared resolver pulls in smol-toml, which only a TOML config on disk
  // can need. Guarding on the same `existsSync` the resolver itself starts
  // with keeps the parser off the common launch path entirely (#720); the
  // resolver skips any individually-missing config on its own.
  if (!configPaths.some((p) => existsSync(p))) {
    return argv;
  }
  const { resolveConfigsExtensionSpecs } = await import(
    "../resolve-config-extensions.js"
  );
  const specs = resolveConfigsExtensionSpecs(configPaths);
  if (specs === null) {
    return argv;
  }
  const flags: string[] = [];
  for (const { path, entrypoint } of specs) {
    flags.push("--extension", entrypoint ? `${path}::${entrypoint}` : path);
  }
  return [...argv, ...flags];
}
