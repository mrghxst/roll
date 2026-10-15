import { expect, test } from '@playwright/test';

// People: name someone, merge two people, take a face out, open their photos.
test('names, merges and corrects people', async ({ page }) => {
  await page.goto('/?setup=1&mock=800&models=1');
  await page.getByTestId('nav-people').click();
  const cards = page.getByTestId('people').locator('li');
  await expect(cards).toHaveCount(4);
  await expect(cards.first()).toContainText('Anna');

  // Name the second person.
  await cards.nth(1).getByTestId('name-person').click();
  await page.getByTestId('person-name').fill('Ben');
  await page.getByTestId('person-name').press('Enter');
  await expect(page.getByTestId('people')).toContainText('Ben');

  // Merge two unnamed people into one.
  const unnamed = cards.filter({ hasText: 'Add a name' });
  await expect(unnamed).toHaveCount(2);
  await unnamed.nth(0).getByRole('checkbox').check();
  await unnamed.nth(1).getByRole('checkbox').check();
  await page.getByTestId('merge-people').click();
  await expect(cards).toHaveCount(3);

  // Not Anna: one face leaves her.
  const anna = cards.filter({ hasText: 'Anna' });
  const before = await anna.locator('.muted').textContent();
  await anna.getByTestId('person-faces').click();
  await page.getByTestId('faces-grid').locator('button').first().click();
  await page.getByTestId('not-person').click();
  await expect(page.getByTestId('faces-grid').locator('button')).toHaveCount(Number(before!.split(' ')[0]) - 1);
  await page.getByRole('button', { name: 'Done' }).click();

  // Her photos in the timeline, with a chip to leave.
  await anna.getByRole('button', { name: 'Photos of Anna' }).click();
  await expect(page.getByTestId('scope-chip')).toContainText('Anna');
});

test('offers the face models when they are not there', async ({ page }) => {
  await page.goto('/?setup=1&mock=200');
  await page.getByTestId('nav-people').click();
  await expect(page.getByTestId('download-faces')).toBeVisible();
});

test('names a face from the info panel', async ({ page }) => {
  await page.goto('/?setup=1&mock=800&models=1');
  await page.getByTestId('nav-people').click();
  await page.getByRole('button', { name: 'Photos of No name yet' }).first().click();
  await page.locator('.tile:not(.hidden)').first().click();
  await page.getByRole('button', { name: 'Info (I)' }).click();
  const info = page.getByTestId('info-people');
  await info.getByTestId('who-is-this').first().click();
  await page.getByRole('combobox', { name: 'Who is this?' }).fill('Cleo');
  await page.getByRole('combobox', { name: 'Who is this?' }).press('Enter');
  await expect(info).toContainText('Cleo');
});
