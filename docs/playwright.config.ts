import { defineConfig, devices } from '@playwright/test'

// Integration tests run against the real built site served via `vitepress
// preview`. The webServer option boots the preview before the suite and
// shuts it down afterwards.
export default defineConfig({
  testDir: './tests/integration',
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? 'list' : 'list',
  use: {
    baseURL: 'http://127.0.0.1:4173/dirsql/',
    trace: 'on-first-retry',
  },
  projects: [
    {
      name: 'chromium',
      use: {
        ...devices['Desktop Chrome'],
        // Respect a prebuilt chromium on the host if provided (e.g. in
        // sandboxed CI images where downloading browsers isn't possible).
        // Falls back to the Playwright-managed browser otherwise.
        ...(process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH
          ? { launchOptions: { executablePath: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH } }
          : {}),
      },
    },
  ],
  // Callers must build first (`pnpm build`). `test:integration` in
  // package.json chains them; CI does the same.
  webServer: {
    command: 'pnpm preview --host 127.0.0.1 --port 4173',
    url: 'http://127.0.0.1:4173/dirsql/',
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
    stdout: 'ignore',
    stderr: 'pipe',
  },
})
