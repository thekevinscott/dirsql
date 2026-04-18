// Resolve the absolute path of the prebuilt `dirsql` binary shipped by
// whichever `@dirsql/cli-<triple>` optional-dependency package matched
// the host at `npm install` time. Never returns if unresolved — die()
// takes over.

import { createRequire } from "node:module";
import { nodeTriples } from "../platforms.js";
import { die } from "./die.js";
import { platformKey } from "./platformKey.js";

/** A minimal `require.resolve`-shaped function. Injectable for tests. */
export type Resolver = (specifier: string) => string;

/** Default resolver: a CJS-style `require.resolve` rooted at this ESM module. */
export function defaultResolver(): Resolver {
  return createRequire(import.meta.url).resolve;
}

export function resolveBinary(
  key = platformKey(),
  resolver: Resolver = defaultResolver(),
): string {
  const triples = nodeTriples();
  const pkg = triples[key];
  if (!pkg) {
    die(
      `no prebuilt binary for ${key}. Build from source with \`cargo install dirsql --features cli\`.`,
    );
  }
  const bin = process.platform === "win32" ? "dirsql.exe" : "dirsql";
  try {
    return resolver(`${pkg}/${bin}`);
  } catch {
    die(
      `${pkg} is not installed. If you ran \`npm install --no-optional\` or \`--ignore-optional\`, re-install without that flag.`,
    );
  }
}
