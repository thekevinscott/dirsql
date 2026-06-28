import { resolve } from "node:path";
import { defineConfig } from "vitest/config";

// Alias `dirsql` to the TS source so vitest instruments it for coverage.
// Without this, tests resolve `dirsql` to the compiled `dist/index.js` via
// the package's self-reference, which v8 coverage can't instrument when
// it's reached through a raw CJS `require()` outside vitest's module graph.
export default defineConfig({
  resolve: {
    alias: {
      dirsql: resolve(__dirname, "src/index.ts"),
    },
  },
  test: {
    // Native napi-rs modules require forks pool (not threads) because
    // the default threads pool uses worker_threads which create a
    // separate V8 context where JS functions have incompatible types
    // for napi_typeof checks.
    pool: "forks",
    // The `docs/` symlink at the package root points to the workspace docs,
    // which contains Playwright e2e specs. Without this exclude, vitest's
    // default test discovery picks them up and tries to load `@playwright/test`.
    exclude: [
      "**/node_modules/**",
      "**/dist/**",
      "**/.{git,cache}/**",
      "docs/**",
    ],
    coverage: {
      provider: "v8",
      include: ["src/**/*.ts", "tools/**/*.ts"],
      exclude: [
        "**/*.test.ts",
        "test/**/*.ts",
        // `dirsql.ts` is the npm `bin` entry shim: it imports `main` and
        // invokes it at module load. Not a function under test, and
        // intentionally not imported anywhere else; the launcher helpers
        // in `main.ts` / `resolveBinary.ts` are unit-tested. Also exempt
        // from the colocated-test gate (see testing-conventions.toml).
        "src/cli/dirsql.ts",
        "src/index.ts", // needs the napi binary; covered by SDK integration tests
      ],
      thresholds: {
        statements: 100,
        lines: 100,
        branches: 100,
        functions: 100,
      },
    },
  },
});
