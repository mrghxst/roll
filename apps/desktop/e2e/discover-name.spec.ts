import { expect, test } from '@playwright/test';

// Discover asks the decision model for a group's name, then offers to
// create the category with it.
test('suggests a name for a Discover group', async ({ page }) => {
  await page.goto('/?setup=1&mock=800&ai=local&model=qwen3-vl:8b-instruct');
  await page.getByTestId('nav-discover').click();
  await page.getByTestId('suggest-name').first().click();
  await expect(page.getByRole('textbox', { name: 'Name' })).toHaveValue(/Concert tickets|Screenshots/);
});

test('hides naming while AI is off', async ({ page }) => {
  await page.goto('/?setup=1&mock=800');
  await page.getByTestId('nav-discover').click();
  await expect(page.getByTestId('assign-cluster').first()).toBeVisible();
  await expect(page.getByTestId('suggest-name')).toHaveCount(0);
});
