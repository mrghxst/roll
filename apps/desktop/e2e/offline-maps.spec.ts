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

  await page.evaluate(() => (window as unknown as W).__rollMap.jumpTo({ center: [10.75, 45.75], zoom: 9 }));
  await page.getByTestId('offline-areas').click();
  await expect(page.getByTestId('area-name')).not.toHaveValue('');
  await page.getByTestId('area-name').fill('Lake Garda');
  await expect(page.getByTestId('keep-area')).toHaveText('Turn on downloads and keep this area');
  await page.getByTestId('keep-area').click();
  const list = page.getByTestId('map-areas');
  await expect(list).toContainText('Lake Garda');
  await expect(list).toContainText('tiles', { timeout: 5000 });
  await page.getByRole('button', { name: 'Remove Lake Garda' }).click();
  await expect(list).toHaveCount(0);
});
