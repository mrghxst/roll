import { expect, test } from '@playwright/test';

// Teach and Find similar, against the mock (`?models=1`: the image model is
// installed; the mock's "looks alike" is the closest colour).
test('teaches a new category from selected examples', async ({ page }) => {
  await page.goto('/?setup=1&mock=400&models=1');
  const tiles = page.locator('.tile:not(.hidden)');
  await tiles.nth(0).click();
  await tiles.nth(1).click({ modifiers: ['Control'] });
  await tiles.nth(2).click({ modifiers: ['Control'] });
  await page.getByTestId('teach').click();
  await expect(page.getByTestId('teach-few')).toBeVisible();
  await page.getByTestId('teach-name').fill('Pokémon GO');
  await page.getByRole('combobox', { name: 'Inside' }).selectOption({ label: 'Screenshots' });
  await page.getByTestId('teach-submit').click();
  await expect(page.getByTestId('toast')).toContainText('Taught Pokémon GO with 3 examples. 40 proposals wait in Review.');
  await page.getByRole('button', { name: 'Open Review' }).click();
  await expect(page.getByRole('navigation', { name: 'Proposed categories' }).getByRole('button', { name: /Pokémon GO/ })).toBeVisible();
});

test('finds similar items, and asks for the model before teaching', async ({ page }) => {
  await page.goto('/?setup=1&mock=400&models=1');
  await page.locator('.tile:not(.hidden)').first().click();
  await page.getByTestId('find-similar').click();
  await expect(page.getByTestId('scope-chip')).toHaveText('Look like the selected item');
  await page.getByRole('button', { name: 'Show everything' }).click();
  await expect(page.getByTestId('scope-chip')).toHaveCount(0);

  await page.goto('/?setup=1&mock=400');
  await page.locator('.tile:not(.hidden)').first().click();
  await expect(page.getByTestId('find-similar')).toHaveCount(0);
  await page.getByTestId('teach').click();
  await expect(page.getByText('Teaching needs the image model')).toBeVisible();
  await expect(page.getByTestId('teach-submit')).toBeDisabled();
});
