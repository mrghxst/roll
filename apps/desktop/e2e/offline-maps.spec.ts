import { expect, test } from '@playwright/test';

type W = { __rollMap: { jumpTo(o: unknown): void; loaded(): boolean } };

// Offline map areas: the whole world is too much; a lake region is kept,
// downloads and can be removed.
test('keeps a map area offline', async ({ page }) => {
  await page.goto('/?setup=1&mock=300&geo=1');
  await page.getByTestId('nav-map').click();
  await page.waitForFunction(() => (window as unknown as W).__rollMap?.loaded());
  await page.evaluate(() => (window as unknown as W).__rollMap.jumpTo({ center: [0, 20], zoom: 1 }));
  await page.getByTestId('offline-areas').click();
  await expect(page.getByTestId('area-too-big')).toBeVisible();
  await page.getByRole('button', { name: 'Done' }).click();

  await page.evaluate(() => (window as unknown as W).__rollMap.jumpTo({ center: [10.75, 45.75], zoom: 11 }));
  await page.getByTestId('offline-areas').click();
  await expect(page.getByTestId('area-name')).not.toHaveValue('');
  await expect(page.getByTestId('area-detail')).toContainText('Every building and footpath');
  await page.getByTestId('area-name').fill('Lake Garda');
  await expect(page.getByTestId('keep-area')).toHaveText('Turn on downloads and keep this area');
  await page.getByTestId('keep-area').click();
  const list = page.getByTestId('map-areas');
  await expect(list).toContainText('Lake Garda');
  await expect(list).toContainText('every building', { timeout: 5000 });
  await page.getByRole('button', { name: 'Remove Lake Garda' }).click();
  await expect(list).toHaveCount(0);
});

// Zoomed in where the map has only country outlines, it says so and
// offers to keep the area; an area at zoom 13 says it has streets only.
test('says when the map has no streets', async ({ page }) => {
  await page.goto('/?setup=1&mock=300&geo=1');
  await page.getByTestId('nav-map').click();
  await page.waitForFunction(() => (window as unknown as W).__rollMap?.loaded());
  await expect(page.getByTestId('map-bare')).toHaveCount(0);
  await expect(page.getByTestId('map-outlines')).toBeVisible();
  await page.evaluate(() => (window as unknown as W).__rollMap.jumpTo({ center: [8.55, 47.37], zoom: 15 }));
  await expect(page.getByTestId('map-bare')).toBeVisible();
  await page.getByRole('button', { name: 'Keep this area offline' }).click();
  await expect(page.getByTestId('area-detail')).toContainText('Every building and footpath');
  await page.getByRole('button', { name: 'Done' }).click();

  await page.evaluate(() => (window as unknown as W).__rollMap.jumpTo({ center: [8.2, 46.8], zoom: 8 }));
  await page.getByTestId('offline-areas').click();
  await expect(page.getByTestId('area-detail')).toContainText('Streets and larger buildings');
});

// The world map: offered while the map has only country outlines,
// downloaded in one click, then the offer goes. The online map is a switch
// in the same sheet and stops the offers.
test('downloads the world map and switches to the online map', async ({ page }) => {
  await page.goto('/?setup=1&mock=300&geo=1');
  await page.getByTestId('nav-map').click();
  await page.waitForFunction(() => (window as unknown as W).__rollMap?.loaded());
  await page.getByRole('button', { name: 'More map detail' }).click();
  await expect(page.getByTestId('world-detail')).toContainText('Lakes, large rivers, motorways');
  await expect(page.getByTestId('get-world')).toHaveText('Turn on downloads and download (63 MB)');
  await page.getByTestId('get-world').click();
  await expect(page.getByRole('button', { name: 'Remove the world map' })).toBeVisible({ timeout: 5000 });
  await page.getByRole('button', { name: 'Done' }).click();
  await expect(page.getByTestId('map-outlines')).toHaveCount(0);
  // Zoomed in, the world map has no streets: that offer still comes.
  await page.evaluate(() => (window as unknown as W).__rollMap.jumpTo({ center: [8.55, 47.37], zoom: 15 }));
  await expect(page.getByTestId('map-bare')).toBeVisible();

  await page.getByTestId('offline-areas').click();
  await page.getByRole('switch', { name: 'Use the online map' }).click();
  await page.getByRole('button', { name: 'Done' }).click();
  await expect(page.getByTestId('map-bare')).toHaveCount(0);
  await page.getByTestId('nav-settings').click();
  await page.getByRole('button', { name: 'Network' }).click();
  await expect(page.getByRole('switch', { name: 'Online map tiles' })).toHaveAttribute('aria-checked', 'true');
});
