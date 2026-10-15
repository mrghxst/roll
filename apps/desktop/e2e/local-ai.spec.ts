import { expect, test } from '@playwright/test';

// One click in Settings › AI: the local AI downloads and becomes the
// decision model; Sort with AI appears.
test('sets up local AI in one click', async ({ page }) => {
  await page.goto('/?setup=1&mock=300');
  await expect(page.getByTestId('nav-ai')).toHaveCount(0);
  await page.getByTestId('nav-settings').click();
  await page.getByRole('button', { name: 'AI', exact: true }).click();
  await page.getByTestId('setup-local-ai').click();
  await expect(page.getByRole('progressbar', { name: 'Downloading the local AI' })).toBeVisible();
  await expect(page.getByTestId('nav-ai')).toBeVisible();
  // Done: the card goes away once the pack is installed.
  await expect(page.getByTestId('local-ai-setup')).toHaveCount(0, { timeout: 15000 });
});
