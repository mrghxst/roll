// Screenshots for the website, from the mock backend with the CC0 test set
// (no real library). Needs the UI dev server on port 1420 (`pnpm dev` in
// apps/desktop), the CC0 set (`node tools/cc0/build.mjs`), the offline map
// areas in fixtures-out/maps (`cargo run -p roll-core --example maparea`,
// the world map included) and ffmpeg for WebP.
//
//   node apps/site/screenshots.mjs [name…]

import { execFileSync } from 'node:child_process';
import { mkdirSync, rmSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const { chromium } = createRequire(join(root, 'apps', 'desktop', 'package.json'))('@playwright/test');
const out = join(root, 'apps', 'site', 'public', 'shots');
const base = 'http://localhost:1420/?setup=1&pics=cc0';
mkdirSync(out, { recursive: true });

const map = async (page, center, zoom) => {
  await page.waitForFunction(() => window.__rollMap?.loaded(), null, { timeout: 30000 });
  await page.evaluate(([c, z]) => window.__rollMap.jumpTo({ center: c, zoom: z }), [center, zoom]);
  await page.waitForTimeout(500);
  await page.waitForFunction(() => window.__rollMap.loaded(), null, { timeout: 30000 });
  await page.waitForTimeout(1200);
};

/** name: [query, theme, steps, what to shoot: the window (default), a CSS selector, or
 * 'content' for a smaller window without the sidebar, so the UI reads larger] */
const SHOTS = {
  'timeline-light': ['&mock=2400', 'light', async () => {}],
  'timeline-dark': ['&mock=2400', 'dark', async () => {}],
  review: ['&mock=2400', 'light', async (p) => p.getByTestId('nav-review').click(), 'content'],
  map: ['&mock=500&geo=1&maps=1', 'dark', async (p) => {
    await p.getByTestId('nav-map').click();
    await map(p, [14, 44], 3.3);
  }],
  'map-street': ['&mock=500&geo=1&maps=1', 'light', async (p) => {
    await p.getByTestId('nav-map').click();
    await map(p, [-9.1395, 38.7115], 16.2);
  }, '.mapwrap'],
  dates: ['&mock=2400', 'light', async (p) => p.getByTestId('nav-dates').click(), 'content'],
  places: ['&mock=2400', 'light', async (p) => p.getByTestId('nav-places').click(), 'content'],
  organize: ['&mock=2400', 'light', async (p) => {
    await p.getByTestId('nav-organize').click();
    await p.getByRole('button', { name: 'Preview plan' }).click();
  }, 'content'],
  ai: ['&mock=2400&ai=local', 'dark', async (p) => p.getByTestId('nav-ai').click(), 'content'],
};

const only = process.argv.slice(2);
const browser = await chromium.launch();
for (const [name, [query, theme, steps, element]] of Object.entries(SHOTS)) {
  if (only.length && !only.includes(name)) continue;
  const content = element === 'content';
  const viewport = content ? { width: 1280, height: 800 } : { width: 1440, height: 900 };
  const page = await browser.newPage({ viewport, deviceScaleFactor: 2, colorScheme: theme });
  await page.goto(base + query);
  await page.locator('.tile').first().waitFor({ timeout: 60000 });
  await steps(page);
  await page.waitForTimeout(1500);
  // No hover states or caret in the picture.
  await page.mouse.move(0, viewport.height - 1);
  const png = join(out, `${name}.png`);
  // The sidebar is 232 px wide (--sidebar).
  if (content) await page.screenshot({ path: png, clip: { x: 232, y: 0, width: viewport.width - 232, height: viewport.height } });
  else if (element) await page.locator(element).screenshot({ path: png });
  else await page.screenshot({ path: png });
  execFileSync('ffmpeg', ['-v', 'error', '-y', '-i', png, '-c:v', 'libwebp', '-quality', '82', join(out, `${name}.webp`)]);
  rmSync(png);
  await page.close();
  console.log(name);
}
await browser.close();
