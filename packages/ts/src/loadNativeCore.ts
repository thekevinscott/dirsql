// Loads the napi-rs native addon. Consumers installing `dirsql` from npm
// pick up a single `@dirsql/lib-<slug>` sub-package via the main package's
// `optionalDependencies`; this loader resolves that package at runtime.
//
// During development (this monorepo, or any local `napi build`) the sub-
// package isn't published yet and `dirsql.node` sits at the package root.
// We fall back to that path so `pnpm test` works from source.

import { createRequire } from "node:module";
import { join } from "node:path";
import { libTriples } from "./platforms.js";

export interface CoreModule {
  DirSQL: unknown;
}

/** `createRequire`-shaped loader. Injectable for tests. */
export type Requirer = (specifier: string) => unknown;

/**
 * Return path of this module's directory. Split out so tests can pin the
 * dev-fallback location without mocking `import.meta`.
 */
export type DirnameFn = () => string;

export function loadNativeCore(
  key = `${process.platform}-${process.arch}`,
  requirer: Requirer = createRequire(import.meta.url),
  dirnameFn: DirnameFn = () => import.meta.dirname,
): CoreModule {
  const libs = libTriples();
  const pkg = libs[key];

  if (pkg) {
    try {
      return requirer(pkg) as CoreModule;
    } catch {
      // Sub-package not installed (dev checkout, or `npm install
      // --no-optional`). Fall through to the dev path.
    }
  }

  const bindingPath = join(dirnameFn(), "..", "dirsql.node");
  return requirer(bindingPath) as CoreModule;
}
