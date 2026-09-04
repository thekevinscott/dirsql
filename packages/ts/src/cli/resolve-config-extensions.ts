// Launcher-side resolution of the TOML configs' `[[dirsql.extension]]` entries.
//
// The compiled `dirsql` binary loads config extensions literally — it has no
// `require.resolve`, so it cannot resolve a bare **package name**. When any
// TOML config in argv names an extension by package name, the shared SDK
// resolver resolves every config's entries and this launcher passes the
// resolved literal paths to the binary via repeatable `--extension` flags;
// the binary then loads those and ignores the configs' own extension entries.
//
// Native-language configs (`.py`/`.js`/`.mjs`/`.cjs`) are untouched: the binary
// dispatches those to `dirsql interpret`, whose handshake already carries
// resolved paths.

import { existsSync } from "node:fs";
import { configPathsFromArgv } from "./config-paths-from-argv.js";

// Config extensions the binary dispatches to `dirsql interpret`; never
// pre-resolved here (that path resolves via the handshake).
const NATIVE_CONFIG_SUFFIXES = [".py", ".js", ".mjs", ".cjs"];

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
