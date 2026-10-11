import { defineConfig, devices } from '@playwright/test';

const baseURL = process.env.RACER_BASE_URL ?? 'http://127.0.0.1:4173';

export default defineConfig({
  testDir: './tests/e2e',
  outputDir: process.env.RACER_TEST_OUTPUT_DIR ?? 'test-results',
  timeout: 60_000, // software GL (SwiftShader/llvmpipe) frames are slow on CI-class hosts
  expect: { timeout: 20_000 }, // polled assertions on slow software-GL frames; failures still surface, just later
  fullyParallel: true,
  workers: 1,
  forbidOnly: Boolean(process.env.CI),
  retries: process.env.CI ? 2 : 1, // one local retry; retried passes are reported as flaky, not hidden
  reporter: 'list',
  // Trace screenshots read back the WebGL canvas (GPU stall per action), slowing every step ~10x on software GL.
  use: { baseURL, trace: { mode: 'retain-on-failure', screenshots: false, snapshots: false } },
  webServer: process.env.RACER_BASE_URL ? undefined : {
    command: 'node scripts/start-dev-server.mjs',
    url: `${baseURL}/healthz`,
    reuseExistingServer: !process.env.CI,
    timeout: 240_000,
  },
  projects: [
    { name: 'chromium', use: { ...devices['Desktop Chrome'], launchOptions: { args: ['--enable-unsafe-webgpu', '--ignore-gpu-blocklist', '--enable-features=Vulkan'] } } },
    { name: 'firefox', use: { ...devices['Desktop Firefox'] } },
  ],
});
