import { expect, test } from '@playwright/test';

// The grid with 100k items (mock backend). Budgets from PROMPT.md 7.5:
// 60 fps scrolling, scrubber jumps show placeholders within 100 ms. Headless
// CI machines are slower than the owner's PC and render WebGL in software,
// so there the asserted limits only catch large regressions; the measured
// numbers are printed. docs/benchmarks.md has the owner's-PC figures.
const CI = !!process.env.CI;
test('scrolls 100k items smoothly and jumps quickly', async ({ page }) => {
  test.setTimeout(90_000);
  await page.goto('/?setup=1&mock=100000');
  const grid = page.getByRole('listbox', { name: 'Photos' });
  await expect(page.locator('.tile:not(.hidden)').first()).toBeVisible({ timeout: 30_000 });

  const frames = await grid.evaluate(async (el) => {
    const times: number[] = [];
    let last = performance.now();
    for (let i = 0; i < 180; i++) {
      el.scrollTop += 90;
      await new Promise((r) => requestAnimationFrame(r));
      const now = performance.now();
      times.push(now - last);
      last = now;
    }
    times.sort((a, b) => a - b);
    return { median: times[90]!, p95: times[171]! };
  });
  console.log(`scroll frames: median ${frames.median.toFixed(1)} ms, p95 ${frames.p95.toFixed(1)} ms`);
  expect(frames.median).toBeLessThan(34);

  const jump = await grid.evaluate(async (el) => {
    const t = performance.now();
    el.scrollTop = el.scrollHeight * 0.73;
    for (;;) {
      await new Promise((r) => requestAnimationFrame(r));
      const box = el.getBoundingClientRect();
      const visible = [...el.querySelectorAll<HTMLElement>('.tile:not(.hidden)')].some((n) => {
        const r = n.getBoundingClientRect();
        return r.bottom > box.top && r.top < box.bottom;
      });
      if (visible) return performance.now() - t;
      if (performance.now() - t > 2000) return 2000;
    }
  });
  console.log(`scrubber jump to placeholders: ${jump.toFixed(0)} ms`);
  expect(jump).toBeLessThan(CI ? 300 : 100);
});

// The map with 100k placed items: budget 60 fps while panning and zooming.
test('pans and zooms a map of 100k points smoothly', async ({ page }) => {
  test.setTimeout(120_000);
  await page.goto('/?setup=1&mock=100000&geo=1');
  await expect(page.locator('.tile:not(.hidden)').first()).toBeVisible({ timeout: 30_000 });
  await page.getByTestId('nav-map').click();
  await expect(page.locator('.roll-pin').first()).toBeVisible({ timeout: 60_000 });
  const frames = await page.evaluate(async () => {
    const map = (window as unknown as { __rollMap: import('maplibre-gl').Map }).__rollMap;
    const times: number[] = [];
    let last = performance.now();
    for (let i = 0; i < 240; i++) {
      if (i % 80 < 40) map.panBy([12, 6], { duration: 0 });
      else map.setZoom(map.getZoom() + (i % 80 < 60 ? 0.04 : -0.04));
      await new Promise((r) => requestAnimationFrame(r));
      const now = performance.now();
      times.push(now - last);
      last = now;
    }
    times.sort((a, b) => a - b);
    return { median: times[120]!, p95: times[228]! };
  });
  console.log(`map frames: median ${frames.median.toFixed(1)} ms, p95 ${frames.p95.toFixed(1)} ms`);
  expect(frames.median).toBeLessThan(CI ? 200 : 34);
});
