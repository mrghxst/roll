import { expect, test } from '@playwright/test';

test('a camera card becomes an event: dates, places, then copy', async ({ page }) => {
  await page.goto('/?setup=1&mock=800');
  await page.getByTestId('nav-add').click();
  await page.getByTestId('add-folder').click();

  // Source: what is in the folder, and what the library already has.
  await expect(page.getByTestId('import-source')).toContainText('42 photos');
  await expect(page.getByTestId('known')).toContainText('2 items are already in your library');
  await expect(page.getByTestId('staged-items').locator('img')).toHaveCount(42);

  // Dates: the phone photos' offset, applied in one click.
  await page.getByTestId('step-dates').click();
  await expect(page.getByTestId('smart-offset')).toContainText('+9 h 28 min 5 s');
  await page.getByTestId('apply-smart').click();
  await expect(page.getByTestId('import-dates')).toContainText('agree with these dates');

  // Places: not before they are checked.
  await page.getByTestId('step-add').click();
  await expect(page.getByTestId('import-add')).toContainText('Check the places first');
  await page.getByTestId('step-places').click();
  await page.getByTestId('accept-place').click();
  await page.getByRole('button', { name: 'Not now' }).click();
  await page.getByTestId('places-ok').click();

  // Event: the proposed name follows the place.
  await expect(page.getByLabel('Event name')).toHaveValue('May 2022 Lisbon');
  await page.getByTestId('next-step').click();

  await expect(page.getByTestId('import-plan')).toContainText('40 files');
  await expect(page.getByTestId('import-plan')).toContainText('2022/2022-05 May 2022 Lisbon');
  await page.getByTestId('apply-import').click();
  await expect(page.getByTestId('import-done')).toContainText('Added 40 items to May 2022 Lisbon');

  // The items are in the library under their event, and undo takes them out.
  await page.getByRole('button', { name: 'Show them' }).click();
  await expect(page.locator('.tile:not(.hidden)').first()).toBeVisible();
  await page.getByTestId('nav-add').click();
  await page.getByRole('button', { name: 'Undo' }).click();
  await expect(page.getByTestId('apply-import')).toBeVisible();
});
