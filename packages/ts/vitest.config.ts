import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    // Native napi-rs modules require forks pool (not threads) because
    // the default threads pool uses worker_threads which create a
    // separate V8 context where JS functions have incompatible types
    // for napi_typeof checks.
    pool: "forks",
    coverage: {
      provider: "v8",
      include: ["dist/index.js"],
      thresholds: {
        lines: 90,
        branches: 90,
        functions: 90,
      },
    },
  },
});
