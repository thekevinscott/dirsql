// Launcher-side resolution of a TOML config's `[[dirsql.extension]]` entries.
//
// The compiled `dirsql` binary loads config extensions literally — it has no
// `require.resolve`, so it cannot resolve a bare **package name**. When a
// TOML config names an extension by package name, the shared SDK resolver
// resolves every entry and this launcher passes the resolved literal paths
// to the binary via repeatable `--extension` flags; the binary then loads
// those and ignores the config's own extension entries.
//
// Native-language configs (`.py`/`.js`/`.mjs`/`.cjs`) are untouched: the binary
// dispatches those to `dirsql interpret`, whose handshake already carries
// resolved paths.

import { existsSync } from "node:fs";

// Config extensions the binary dispatches to `dirsql interpret`; never
// pre-resolved here (that path resolves via the handshake).
const NATIVE_CONFIG_SUFFIXES = [".py", ".js", ".mjs", ".cjs"];

/** The `--config` value from argv (`--config X` or `--config=X`), or the default. */
function configPathFromArgv(argv: string[]): string {
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--config") {
      return argv[i + 1] ?? "";
    }
    if (a?.startsWith("--config=")) {
      return a.slice("--config=".length);
    }
  }
  return "./.dirsql.toml";
}

/**
 * Return `argv` augmented with `--extension <path>[::entrypoint]` flags when the
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
  const configPath = configPathFromArgv(argv);
  if (NATIVE_CONFIG_SUFFIXES.some((s) => configPath.endsWith(s))) {
    return argv;
  }
  // The shared resolver pulls in smol-toml, which only a TOML config on disk
  // can need. Guarding on the same `existsSync` the resolver itself starts
  // with keeps the parser off the common launch path entirely (#720).
  if (!existsSync(configPath)) {
    return argv;
  }
  const { resolveConfigExtensionSpecs } = await import(
    "../resolve-config-extensions.js"
  );
  const specs = resolveConfigExtensionSpecs(configPath);
  if (specs === null) {
    return argv;
  }
  const flags: string[] = [];
  for (const { path, entrypoint } of specs) {
    flags.push("--extension", entrypoint ? `${path}::${entrypoint}` : path);
  }
  return [...argv, ...flags];
}
