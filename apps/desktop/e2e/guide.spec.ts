import { expect, test } from '@playwright/test';

test('the first guide opens after indexing, tracks the steps, and closes', async ({ page }) => {
  await page.goto('/?setup=1&mock=400&guide=1');
  await expect(page.getByRole('heading', { name: 'Get started' })).toBeVisible();
  await expect(page.getByText(/of 7 done/)).toBeVisible();

  // Opening a step goes to its view and counts it.
  const before = Number((await page.getByText(/of 7 done/).textContent())!.split(' ')[0]);
  await page.getByTestId('guide-discover').getByRole('button', { name: 'Open' }).click();
  await expect(page.getByRole('heading', { name: 'Discover' })).toBeVisible();
  await page.getByTestId('nav-guide').click();
  await expect(page.getByText(`${before + 1} of 7 done`)).toBeVisible();

  await page.getByTestId('guide-immich').getByRole('button', { name: 'Mark as done' }).click();
  await expect(page.getByText(`${before + 2} of 7 done`)).toBeVisible();

  await page.getByTestId('guide-close').click();
  await expect(page.getByTestId('nav-guide')).toHaveCount(0);
});
