import { expect, test } from '@playwright/test';

// Merge all exact copies: a preview first, then one step with one undo.
test('merges all exact copies after a preview, and undoes them at once', async ({ page }) => {
  await page.goto('/?setup=1&mock=800');
  await page.getByTestId('nav-duplicates').click();
  await page.getByTestId('merge-exact').click();
  const plan = page.getByTestId('exact-plan');
  await expect(plan).toContainText("copies in 5 groups go to Roll's trash");
  await expect(page.locator('.plan li')).toHaveCount(5);
  await page.getByTestId('merge-exact-confirm').click();
  await expect(page.getByTestId('toast')).toContainText('Moved 5 exact copies to the trash');
  await expect(page.getByTestId('merge-exact')).toHaveCount(0);
  await page.getByRole('button', { name: 'Undo' }).click();
  await expect(page.getByTestId('merge-exact')).toBeVisible();
});

// Best shots of a series: ranked best first, the best suggested, several
// can be kept, the rest go to the trash.
test('picks the best shots of a burst', async ({ page }) => {
  await page.goto('/?setup=1&mock=800');
  await page.getByTestId('nav-duplicates').click();
  const burst = page.locator('li').filter({ has: page.getByTestId('ranked') }).first();
  await expect(burst.getByTestId('ranked')).toContainText('Best first, by sharpness and exposure');
  await expect(burst.locator('.notes').first()).toHaveText('Sharpest · Best exposed');
  await expect(burst.getByRole('button', { name: 'Kept' })).toHaveCount(1);
  // Keep a second frame too.
  await burst.getByRole('button', { name: 'Choose to keep this one' }).first().click();
  await expect(burst.getByTestId('keep-best')).toHaveText('Keep 2, trash 2');
  await burst.getByTestId('keep-best').click();
  await expect(page.getByTestId('toast')).toContainText('Moved 2 duplicates to the trash');
});

// Closed eyes: the eye models are offered next to the series, downloaded
// in one click, and the series are ranked again with them.
test('downloads the eye models and ranks blinks last', async ({ page }) => {
  await page.goto('/?setup=1&mock=800&models=1');
  await page.getByTestId('nav-duplicates').click();
  const setup = page.getByTestId('eyes-setup');
  await expect(setup).toContainText('about 5.7 MB');
  await expect(page.getByTestId('download-eyes')).toHaveText('Turn on downloads and download (5.7 MB)');
  await page.getByTestId('download-eyes').click();
  await expect(setup).toHaveCount(0, { timeout: 10_000 });
  const burst = page.locator('li').filter({ has: page.getByTestId('ranked') }).first();
  await expect(burst.getByTestId('ranked')).toContainText('by sharpness, exposure, faces, closed eyes and look');
});
