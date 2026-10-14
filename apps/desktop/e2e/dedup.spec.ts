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
