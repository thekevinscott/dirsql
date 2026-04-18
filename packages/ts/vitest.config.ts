import { resolve } from "node:path";
import { defineConfig } from "vitest/config";

// Alias `dirsql` to the TS source so vitest instruments it for coverage.
// Without this, tests resolve `dirsql` to the compiled `dist/index.js` via
// the package's self-reference, which v8 coverage can't instrument when
// it's reached through a raw CJS `require()` outside vitest's module graph.
export default defineConfig({
  resolve: {
    alias: {
      dirsql: resolve(__dirname, "ts/index.ts"),
    },
  },
  test: {
    // Native napi-rs modules require forks pool (not threads) because
    // the default threads pool uses worker_threads which create a
    // separate V8 context where JS functions have incompatible types
    // for napi_typeof checks.
    pool: "forks",
    coverage: {
      provider: "v8",
      include: ["ts/**/*.ts", "tools/**/*.ts"],
      exclude: [
        "**/*.test.ts",
        "test/**/*.ts",
        "ts/bin/dirsql.ts", // 5-line entry, exercised by the integration test
        "ts/index.ts", // needs the napi binary; covered by SDK integration tests
      ],
      thresholds: {
        lines: 90,
        branches: 90,
        functions: 90,
      },
    },
  },
});
