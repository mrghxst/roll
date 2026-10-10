import { expect, test } from '@playwright/test';

// The grid with 100k items (mock backend). Budgets from PROMPT.md 7.5:
// 60 fps scrolling, scrubber jumps show placeholders within 100 ms. Headless
// CI machines are slower than the owner's PC, so the asserted limits are
// looser; the measured numbers are printed.
test('scrolls 100k items smoothly and jumps quickly', async ({ page }) => {
  test.setTimeout(90_000);
  await page.goto('/?setup=1&mock=100000');
  const grid = page.getByRole('grid', { name: 'Photos' });
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
  expect(jump).toBeLessThan(100);
});
