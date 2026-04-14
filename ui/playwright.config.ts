import { defineConfig, devices } from '@playwright/test'

export default defineConfig({
  testDir: './e2e',
  timeout: 60_000,
  retries: 0,
  reporter: [['list']],
  use: {
    baseURL: 'http://localhost:5173',
    headless: true,
    ignoreHTTPSErrors: true,
  },
  projects: [
    { name: 'chromium', use: { ...devices['Desktop Chrome'] } },
  ],
  // Start daemon + Vite before all tests
  webServer: [
    {
      command: '/Users/admin/.cargo/bin/cargo run --manifest-path ../daemon/Cargo.toml --bin hush',
      url: 'https://localhost:9111/health',
      timeout: 30_000,
      reuseExistingServer: true,
      ignoreHTTPSErrors: true,
    },
    {
      command: 'npm run dev',
      url: 'http://localhost:5173',
      timeout: 30_000,
      reuseExistingServer: true,
    },
  ],
})
