import { expect, test } from '@playwright/test';

test.beforeEach(async ({ page }) => {
  await page.goto('/?setup=1&mock=800');
  await page.getByTestId('nav-events').click();
  await expect(page.getByTestId('proposed-events').getByTestId('event').first()).toBeVisible();
});

test('confirming an event tags its items and shows them', async ({ page }) => {
  const first = page.getByTestId('proposed-events').getByTestId('event').first();
  const name = (await first.locator('strong').textContent())!;
  await first.getByTestId('confirm-event').click();
  await expect(page.getByTestId('toast')).toContainText(`Confirmed ${name}`);
  const confirmed = page.getByTestId('confirmed-events').getByTestId('event');
  await expect(confirmed).toHaveCount(1);
  await expect(page.getByRole('navigation', { name: 'Sections' })).toContainText(`Event/${name}`);

  await confirmed.getByRole('button', { name: 'Show items' }).click();
  await expect(page.locator('.tile:not(.hidden)').first()).toBeVisible();
});

test('rename and confirm, split, and merge', async ({ page }) => {
  const proposed = page.getByTestId('proposed-events').getByTestId('event');
  const before = await proposed.count();
  expect(before).toBeGreaterThan(2);

  await proposed.first().getByRole('button', { name: 'Rename and confirm…' }).click();
  await page.getByLabel('Event name').fill('Summer at the lake');
  await page.getByTestId('save-event-name').click();
  const confirmed = page.getByTestId('confirmed-events').getByTestId('event');
  await expect(confirmed.first()).toContainText('Summer at the lake');

  // Split the confirmed event at its third item.
  await confirmed.first().getByRole('button', { name: 'Split…' }).click();
  const sheet = page.getByRole('dialog', { name: /^Split/ });
  await sheet.getByRole('button', { name: 'Start the new event here' }).nth(2).click();
  await sheet.getByLabel('Name of the new event').fill('Second week');
  await sheet.getByTestId('split-event').click();
  await expect(confirmed).toHaveCount(2);
  await expect(page.getByTestId('confirmed-events')).toContainText('Second week');

  // Merge two suggestions.
  const n = await proposed.count();
  await proposed.nth(0).getByRole('checkbox').check();
  await proposed.nth(1).getByRole('checkbox').check();
  await page.getByTestId('merge-events').click();
  await expect(proposed).toHaveCount(n - 1);
});

test('dismissing a suggestion hides it', async ({ page }) => {
  const proposed = page.getByTestId('proposed-events').getByTestId('event');
  const n = await proposed.count();
  await proposed.first().getByRole('button', { name: 'Not an event' }).click();
  await expect(proposed).toHaveCount(n - 1);
});
