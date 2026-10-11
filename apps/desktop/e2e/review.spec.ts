import { expect, test } from '@playwright/test';

test.beforeEach(async ({ page }) => {
  await page.goto('/?setup=1&mock=600');
  await expect(page.locator('.tile:not(.hidden)').first()).toBeVisible();
});

test('reviews proposals with the keyboard', async ({ page }) => {
  await page.getByTestId('nav-review').click();
  const progress = page.getByTestId('review-progress');
  await expect(progress).toContainText('Left');
  const left = async () => Number((await progress.textContent())!.match(/Left ([\d,]+)/)![1]!.replace(/,/g, ''));
  const start = await left();
  const cards = page.getByTestId('review-grid').getByRole('option');
  await expect(cards.first()).toBeVisible();
  // Most doubtful first.
  const confs = await cards.locator('.conf').allTextContents();
  const nums = confs.map((c) => Number(c.replace('%', '')));
  expect([...nums].sort((a, b) => a - b)).toEqual(nums);
  // 2 moves the focused item to the second category.
  await page.keyboard.press('2');
  await expect(page.getByTestId('toast')).toContainText('Moved to');
  await expect.poll(left).toBe(start - 1);
  // Z undoes it.
  await page.keyboard.press('z');
  await expect(page.getByTestId('toast')).toContainText('Undoing');
  await expect.poll(left).toBe(start);
  // Enter accepts the page.
  const page1 = await cards.count();
  await page.keyboard.press('Enter');
  await expect(page.getByTestId('toast')).toContainText('Accepted');
  await expect.poll(left).toBe(start - page1);
});

test('accepts everything at 95% or higher in one action', async ({ page }) => {
  await page.getByTestId('nav-review').click();
  await page.getByTestId('accept-high').click();
  await expect(page.getByTestId('toast')).toContainText(/Accepted|No proposals/);
});

test('merges duplicates and dismisses a group', async ({ page }) => {
  await page.getByTestId('nav-duplicates').click();
  const groups = page.getByTestId('dupe-groups').locator('li');
  await expect(groups.first()).toBeVisible();
  const before = await groups.count();
  // Choose the second item to keep, then merge.
  await groups.first().locator('.item').nth(1).click();
  await expect(groups.first().locator('.item').nth(1)).toHaveAttribute('aria-pressed', 'true');
  await groups.first().getByTestId('merge').click();
  await expect(page.getByTestId('toast')).toContainText('to the trash');
  await expect(groups).toHaveCount(before - 1);
  await groups.first().getByRole('button', { name: 'Not duplicates' }).click();
  await expect(groups).toHaveCount(before - 2);
});
