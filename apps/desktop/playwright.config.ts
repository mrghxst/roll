import { defineConfig, devices } from '@playwright/test';

const gpu = !process.env.CI && process.platform === 'win32';
const GPU_ARGS = ['--enable-gpu', '--use-angle=d3d11', '--ignore-gpu-blocklist'];

export default defineConfig({
  testDir: './e2e',
  timeout: 30_000,
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? 'github' : 'list',
  use: {
    baseURL: 'http://localhost:1420',
    viewport: { width: 1440, height: 900 },
    trace: 'retain-on-failure',
  },
  projects: [
    { name: 'chromium', testIgnore: /perf\.spec\.ts/, use: { ...devices['Desktop Chrome'], viewport: { width: 1440, height: 900 } } },
    // Frame timings need a quiet machine: after everything else, one at a time.
    {
      name: 'perf',
      testMatch: /perf\.spec\.ts/,
      dependencies: ['chromium'],
      fullyParallel: false,
      // Locally on Windows, Edge draws WebGL on the graphics card like the
      // app's WebView2; Playwright's own Chromium draws it in software, where
      // the globe takes several times longer than on any real machine.
      use: { ...devices['Desktop Chrome'], viewport: { width: 1440, height: 900 }, ...(gpu ? { channel: 'msedge', launchOptions: { args: GPU_ARGS } } : {}) },
    },
  ],
  webServer: {
    command: 'node node_modules/vite/bin/vite.js --port 1420 --strictPort',
    port: 1420,
    reuseExistingServer: !process.env.CI,
  },
});
