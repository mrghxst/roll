import { expect, test } from '@playwright/test';

test('a model downloads only after the owner confirms size, source and license', async ({ page }) => {
  await page.goto('/?setup=1&mock=50');
  await page.getByTestId('nav-settings').click();
  await page.getByRole('button', { name: 'Models and data' }).click();
  const model = page.locator('[data-pack=image-model]');
  await expect(model).toContainText('1.4 GB');
  await model.getByTestId('download-image-model').click();

  const sheet = page.getByRole('dialog', { name: 'Download image understanding' });
  await expect(sheet).toContainText('huggingface.co');
  await expect(sheet).toContainText('Apache-2.0');
  await expect(sheet).toContainText('SHA-256');
  // Downloads are off until the owner turns them on; the button says so.
  await expect(sheet.getByTestId('confirm-download')).toHaveText(/Turn on downloads and download 1\.4 GB/);
  await sheet.getByTestId('confirm-download').click();
  await expect(sheet).toBeHidden();

  await expect(model.getByRole('progressbar')).toBeVisible();
  await expect(model).toContainText('Installed', { timeout: 10_000 });
  await page.getByRole('button', { name: 'Network', exact: true }).click();
  await expect(page.getByRole('switch', { name: 'Downloads of models, maps and place names' })).toBeChecked();
});

test('place names download from the Places page', async ({ page }) => {
  await page.goto('/?setup=1&mock=50&places=0');
  await page.getByTestId('nav-places').click();
  await page.getByTestId('download-places').click();
  const sheet = page.getByRole('dialog', { name: 'Download place names' });
  await expect(sheet).toContainText('download.geonames.org');
  await expect(sheet).toContainText('no fixed checksum');
  await sheet.getByTestId('confirm-download').click();
  await expect(page.getByTestId('download-places')).toBeHidden({ timeout: 10_000 });
  await expect(page.getByText('Place names are not installed')).toBeHidden();
});
