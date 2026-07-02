// Launcher-side resolution of a TOML config's `[[dirsql.extension]]` entries.
//
// The compiled `dirsql` binary reads a `.dirsql.toml` itself and loads its
// extensions literally — it has no `require.resolve`, so it cannot resolve a
// bare **package name** (#227). This launcher can. When a TOML config names an
// extension by package name, we resolve every one of its extensions here and
// pass the resolved literal paths to the binary via repeatable `--extension`
// flags; the binary then loads those and ignores the config's own extension
// entries (see the Rust `--extension` flag / `suppress_config_extensions`).
//
// Native-language configs (`.py`/`.js`/`.mjs`/`.cjs`) are untouched: the binary
// dispatches those to `dirsql interpret`, whose handshake already carries
// resolved paths (`toJSON()` runs the same resolver).

import { existsSync, readFileSync } from "node:fs";
import { dirname, resolve as resolvePath } from "node:path";
import { parse as parseToml } from "smol-toml";
import { isBareName, resolveExtensionPath } from "../resolve-extension.js";

// Config extensions the binary dispatches to `dirsql interpret`; never
// pre-resolved here (that path resolves via the handshake).
const NATIVE_CONFIG_SUFFIXES = [".py", ".js", ".mjs", ".cjs"];

// biome-ignore lint/suspicious/noExplicitAny: TOML root has a dynamic shape.
type Toml = Record<string, any>;

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
export function withResolvedExtensions(argv: string[]): string[] {
  if (argv[0] === "init") {
    return argv;
  }
  const configPath = configPathFromArgv(argv);
  if (NATIVE_CONFIG_SUFFIXES.some((s) => configPath.endsWith(s))) {
    return argv;
  }
  if (!existsSync(configPath)) {
    return argv;
  }

  let doc: Toml;
  try {
    doc = parseToml(readFileSync(configPath, "utf8")) as Toml;
  } catch {
    // Leave a malformed config for the binary to report.
    return argv;
  }
  const dirsql = (doc.dirsql ?? {}) as Toml;
  const entries: Toml[] = Array.isArray(dirsql.extension)
    ? dirsql.extension
    : [];
  if (entries.length === 0) {
    return argv;
  }
  // Only intervene when at least one path is a bare package name; a config with
  // only literal paths keeps the binary's existing behavior untouched.
  const hasPackageName = entries.some(
    (e) => typeof e.path === "string" && isBareName(e.path),
  );
  if (!hasPackageName) {
    return argv;
  }

  const base = dirname(resolvePath(configPath));
  const flags: string[] = [];
  for (const e of entries) {
    const path = resolveExtensionPath(e.path as string, base, true);
    const entrypoint = typeof e.entrypoint === "string" ? e.entrypoint : null;
    flags.push("--extension", entrypoint ? `${path}::${entrypoint}` : path);
  }
  return [...argv, ...flags];
}
