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
      // e2e (`tests/e2e/`) and smoke (`tests/smoke/`) run via their own
      // `pnpm test:e2e` / `pnpm test:smoke` scripts (which depend on
      // `pnpm build`). Excluding them keeps the default `pnpm test` and
      // `test:integration` fast and free of the cargo-build prerequisite.
      "tests/e2e/**",
      "tests/smoke/**",
    ],
    coverage: {
      provider: "v8",
      include: ["src/**/*.ts", "tools/**/*.ts"],
      exclude: [
        "**/*.test.ts",
        "tests/**/*.ts",
        // `index.ts` is a true re-export barrel (its logic was extracted
        // into the colocated-tested table.ts / core.ts / parse-table-name.ts
        // / dirsql.ts modules). It needs the napi binary when loaded for
        // real and is covered by the SDK integration tests; exempt from the
        // colocated-test gate too (see testing-conventions.toml).
        "src/index.ts",
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
