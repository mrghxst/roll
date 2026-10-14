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
  await expect(burst.getByTestId('ranked')).toContainText('Best first, by sharpness, exposure');
  await expect(burst.locator('.notes').first()).toHaveText('Sharpest · Best exposed');
  await expect(burst.getByRole('button', { name: 'Kept' })).toHaveCount(1);
  // Keep a second frame too.
  await burst.getByRole('button', { name: 'Choose to keep this one' }).first().click();
  await expect(burst.getByTestId('keep-best')).toHaveText('Keep 2, trash 2');
  await burst.getByTestId('keep-best').click();
  await expect(page.getByTestId('toast')).toContainText('Moved 2 duplicates to the trash');
});
