import { expect, test } from '@playwright/test';

test('Enter turns understood words into chips', async ({ page }) => {
  await page.goto('/?setup=1&mock=600');
  await expect(page.locator('.tile:not(.hidden)').first()).toBeVisible();
  const search = page.getByTestId('search');
  await search.fill('videos 2019 beach');
  await search.press('Enter');
  await expect(page.locator('.chip', { hasText: 'Videos' })).toBeVisible();
  await expect(page.locator('.chip', { hasText: '2019' })).toBeVisible();
  await expect(search).toHaveValue('beach');
  // Removing a chip widens the search again.
  await page.getByRole('button', { name: 'Remove filter 2019' }).click();
  await expect(page.locator('.chip', { hasText: '2019' })).toHaveCount(0);
});
