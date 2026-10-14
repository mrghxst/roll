import { expect, test } from '@playwright/test';

// The smart offset in Dates: phone photos of the same moments give the
// camera's clock error, with the pairs as evidence, applied in one click.
test('dates offers the camera offset from phone photos of the same moments', async ({ page }) => {
  await page.goto('/?setup=1&mock=800');
  await page.getByTestId('nav-dates').click();
  const card = page.getByTestId('smart-offset');
  await expect(card).toContainText('Phone photos of the same moments suggest the camera clock is off by');
  await expect(card.locator('.pair')).toHaveCount(4);
  await page.getByTestId('apply-smart').click();
  await expect(page.getByTestId('toast')).toContainText('Fixed the date of 24 items');
  await expect(page.getByText('The folder says 2019')).toHaveCount(0);
});
