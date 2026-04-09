import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    coverage: {
      provider: "v8",
      include: ["index.js"],
      thresholds: {
        lines: 90,
        branches: 90,
        functions: 90,
      },
    },
  },
});
