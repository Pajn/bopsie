import { defineConfig, devices } from '@playwright/test';

/**
 * Smoke tests of the web UI as the server serves it, at a desktop's width and a phone's,
 * against the throwaway test server (mobile/scripts/test-server.sh), whose image carries this
 * checkout's web UI. Without TEST_SERVER_URL they are skipped.
 */
export default defineConfig({
  testDir: 'e2e',
  // The tests share the server's players and queues, so they take turns.
  workers: 1,
  fullyParallel: false,
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? [['github'], ['html', { open: 'never' }]] : 'list',
  globalSetup: './e2e/global-setup.ts',
  use: {
    baseURL: process.env.TEST_SERVER_URL,
    trace: 'retain-on-failure',
  },
  projects: [
    {
      name: 'desktop',
      use: { ...devices['Desktop Chrome'], viewport: { width: 1280, height: 800 } },
    },
    { name: 'phone', use: { ...devices['Pixel 7'] } },
  ],
});
