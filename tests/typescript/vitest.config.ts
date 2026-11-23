import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    // napi modules need forks, as in packages/ts.
    pool: "forks",
    include: [".harnessmd/**/*.test.ts"],
    setupFiles: process.env.MEASURE ? ["measure.ts"] : [],
  },
});
