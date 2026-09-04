**Changed**

- **The launcher's argv config-path scan moved to its own module.** `configPathsFromArgv` now lives in `src/cli/config-paths-from-argv.ts`; `withResolvedExtensions` imports it. Internal reorganisation with no change to the published API or CLI behavior. (#1037)
