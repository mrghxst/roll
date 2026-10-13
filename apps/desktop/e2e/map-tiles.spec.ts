import { expect, test } from '@playwright/test';

// The tile host is intercepted: the test never reaches OpenFreeMap.
test('online map tiles load only after they are turned on', async ({ page }) => {
  const requests: string[] = [];
  await page.route('https://tiles.openfreemap.org/**', async (route) => {
    const url = route.request().url();
    requests.push(url);
    if (url.endsWith('/planet')) {
      await route.fulfill({
        json: { tilejson: '3.0.0', tiles: ['https://tiles.openfreemap.org/planet/t/{z}/{x}/{y}.pbf'], minzoom: 0, maxzoom: 14 },
      });
    } else {
      await route.fulfill({ status: 204 });
    }
  });

  const styleErrors: string[] = [];
  page.on('console', (m) => {
    if (m.type() === 'error' && /layers\.|source|glyphs/i.test(m.text())) styleErrors.push(m.text());
  });

  await page.goto('/?setup=1&mock=200');
  await page.getByTestId('nav-map').click();
  await page.waitForFunction(() => (window as unknown as { __rollMap?: { loaded(): boolean } }).__rollMap?.loaded());
  const hasDetail = () => page.evaluate(() => !!(window as unknown as { __rollMap: { getSource(id: string): unknown } }).__rollMap.getSource('detail'));
  expect(await hasDetail()).toBe(false);
  expect(requests).toEqual([]);

  await page.getByTestId('nav-settings').click();
  await page.getByRole('button', { name: 'Network', exact: true }).click();
  await page.getByRole('switch', { name: 'Online map tiles' }).click();
  await page.getByTestId('nav-map').click();
  // The map is built again on return; without a GPU the globe takes a while.
  await expect.poll(hasDetail, { timeout: 20_000 }).toBe(true);
  await expect.poll(() => requests.some((u) => u.endsWith('/planet'))).toBe(true);
  // Detail sits below the items: the dots stay on top.
  const order = await page.evaluate(() => {
    const ids = (window as unknown as { __rollMap: { getStyle(): { layers: { id: string }[] } } }).__rollMap.getStyle().layers.map((l) => l.id);
    return [ids.indexOf('d-road'), ids.indexOf('dots')];
  });
  expect(order[0]).toBeGreaterThan(-1);
  expect(order[0]).toBeLessThan(order[1]!);
  expect(styleErrors).toEqual([]);

  await page.getByTestId('nav-settings').click();
  await page.getByRole('button', { name: 'Network', exact: true }).click();
  await page.getByRole('switch', { name: 'Online map tiles' }).click();
  await page.getByTestId('nav-map').click();
  await expect.poll(hasDetail, { timeout: 20_000 }).toBe(false);
});
