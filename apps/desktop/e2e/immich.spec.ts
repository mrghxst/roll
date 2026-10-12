import { expect, test } from '@playwright/test';

test('connect, sync the plan, keep one item local and remove it after confirming', async ({ page }) => {
  await page.goto('/?setup=1&mock=800');
  await page.getByTestId('nav-immich').click();
  await expect(page.getByTestId('immich-status')).toContainText('Add your server address');

  // Settings: address and key (the key goes to the keychain).
  await page.getByRole('button', { name: 'Open Settings' }).click();
  await page.getByRole('navigation', { name: 'Settings sections' }).getByRole('button', { name: 'Immich' }).click();
  await page.getByPlaceholder('https://photos.example.com').fill('https://photos.example.com');
  await page.getByPlaceholder('https://photos.example.com').blur();
  await page.getByPlaceholder('Needs access to assets, albums and tags').fill('secret');
  await page.getByRole('button', { name: 'Save', exact: true }).click();

  // Nothing reaches the server until Immich is on.
  await page.getByTestId('nav-immich').click();
  await expect(page.getByTestId('immich-status')).toContainText('Immich is off');
  await page.getByTestId('immich-on').click();
  await expect(page.getByTestId('immich-status')).toContainText('Connected to Immich 3.3.1');

  const plan = page.getByTestId('immich-plan');
  const sends = Number((await plan.locator('li').first().locator('strong').textContent())!.replace(/\D/g, ''));
  expect(sends).toBeGreaterThan(0);
  await page.getByTestId('immich-sync').click();
  await expect(plan.locator('li').first()).toContainText('0 to send');
  await expect(plan).toContainText(`${sends.toLocaleString('en-US')} items already up to date`);

  // One photo: Keep local only.
  await page.getByTestId('nav-timeline').click();
  await page.getByRole('navigation', { name: 'Sections' }).getByRole('button', { name: /^Photos/ }).click();
  await page.locator('.tile:not(.hidden)').first().click();
  await page.getByTestId('immich-menu').click();
  await page.getByRole('menuitem', { name: 'Keep local only' }).click();
  await page.getByTestId('nav-immich').click();
  await expect(plan).toContainText('1 item is in Immich but now marked Keep local only');
  await expect(page.getByTestId('immich-sync')).toBeDisabled();
  await page.getByRole('switch', { name: "Move them to Immich's trash" }).click();
  await page.getByTestId('immich-sync').click();
  await expect(plan).not.toContainText('Keep local only');

  await page.getByTestId('nav-history').click();
  await expect(page.getByText("Immich: moved 1 to Immich's trash")).toBeVisible();
});
