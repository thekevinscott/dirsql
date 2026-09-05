import { loadNativeCore } from "../load-native-core.js";

/** The addon's CLI entry point, as napi exports it. */
export type RunCli = (argv: readonly string[]) => number;

/**
 * Resolve `runCli` off the same addon the SDK loads.
 *
 * bin-shim can resolve an addon itself, but its default naming is
 * `@{scope}/lib-{platform}-{arch}`, which cannot express the ABI suffix
 * dirsql's packages carry (`@dirsql/lib-linux-x64-gnu`). Reusing
 * `loadNativeCore` keeps one resolution rule for the SDK and the CLI —
 * including its dev fallback to a locally built `dirsql.node` — so a
 * monorepo checkout and a published install take the same path.
 */
export function resolveRunCli(): RunCli {
  const core = loadNativeCore() as { runCli?: unknown };
  if (typeof core.runCli !== "function") {
    throw new Error(
      "dirsql: the native addon has no callable `runCli` export; " +
        "it was built without the `cli` feature.",
    );
  }
  return core.runCli as RunCli;
}
