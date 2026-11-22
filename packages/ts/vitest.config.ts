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
      // e2e (`tests/e2e/`) runs via its own `pnpm test:e2e` script (which
      // depends on `pnpm build`). Excluding it keeps the default `pnpm test`
      // and `test:integration` fast and free of the cargo-build prerequisite.
      // (Packaging distcheck moved out of this package entirely -- #520, now the
      // `internals/distcheck` package.)
      "tests/e2e/**",
    ],
    // Coverage shape for local `pnpm coverage` runs. The enforced unit-only
    // floor lives in testing-conventions.toml `[typescript.coverage]` and is
    // gated in CI by `testing-conventions unit coverage` (ts-test.yml), which
    // passes its own include/exclude -- so there is no bespoke threshold here.
    coverage: {
      provider: "v8",
      include: ["src/**/*.ts", "tools/**/*.ts"],
      exclude: ["**/*.test.ts", "tests/**/*.ts"],
    },
  },
});
