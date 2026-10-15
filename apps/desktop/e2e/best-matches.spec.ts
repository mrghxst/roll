import { expect, test } from '@playwright/test';

// A search's best matches in order, then "Better ranking" by the decision
// model with its scores.
test('shows best matches and ranks them with the model', async ({ page }) => {
  await page.goto('/?setup=1&mock=800&ai=local&model=qwen3-vl:8b-instruct');
  await expect(page.getByTestId('best-matches')).toHaveCount(0);
  await page.getByTestId('search').fill('img');
  await page.getByTestId('best-matches').click();
  await expect(page.getByTestId('best-lead')).toContainText('match “img” best');
  await expect(page.getByTestId('best-grid').locator('button')).toHaveCount(50);
  await page.getByTestId('better-ranking').click();
  await expect(page.getByTestId('best-lead')).toContainText('as the model rated them');
  const scores = await page.getByTestId('best-grid').locator('.score').allTextContents();
  expect(scores).toHaveLength(50);
  const nums = scores.map(Number);
  expect(nums).toEqual([...nums].sort((a, b) => b - a));
});

test('offers no model ranking while AI is off', async ({ page }) => {
  await page.goto('/?setup=1&mock=800');
  await page.getByTestId('search').fill('img');
  await page.getByTestId('best-matches').click();
  await expect(page.getByTestId('best-grid').locator('button').first()).toBeVisible();
  await expect(page.getByTestId('better-ranking')).toHaveCount(0);
});
