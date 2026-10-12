import { expect, test } from '@playwright/test';

test.describe('first run', () => {
  test('walks through the steps and opens the timeline', async ({ page }) => {
    await page.goto('/?mock=300');
    await expect(page.getByRole('heading', { name: 'Welcome to Roll' })).toBeVisible();
    await page.getByTestId('continue').click();
    await expect(page.getByRole('heading', { name: 'Choose the library folder' })).toBeVisible();
    await expect(page.getByTestId('continue')).toBeDisabled();
    await page.getByTestId('choose-folder').click();
    await page.getByTestId('continue').click();
    await expect(page.getByRole('heading', { name: 'How to save changes' })).toBeVisible();
    await page.getByRole('radio', { name: /Sidecars only/ }).click();
    await page.getByTestId('continue').click();
    await expect(page.getByRole('heading', { name: 'Choose AI' })).toBeVisible();
    await page.getByTestId('continue').click();
    await page.getByRole('button', { name: 'Skip' }).click();
    await expect(page.getByRole('heading', { name: 'Ready' })).toBeVisible();
    await expect(page.getByText('Sidecars only')).toBeVisible();
    await page.getByTestId('continue').click();
    await expect(page.getByRole('listbox', { name: 'Photos' })).toBeVisible();
    await expect(page.locator('.tile:not(.hidden)').first()).toBeVisible();
  });
});

test.describe('library', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/?setup=1&mock=600');
    await expect(page.locator('.tile:not(.hidden)').first()).toBeVisible();
  });

  test('selects with click, shift-click, Ctrl+A and Escape', async ({ page }) => {
    const tiles = page.locator('.tile:not(.hidden)');
    await tiles.nth(0).click();
    await expect(page.getByTestId('selection-bar')).toContainText('1 selected');
    await tiles.nth(3).click({ modifiers: ['Shift'] });
    await expect(page.getByTestId('selection-bar')).toContainText('4 selected');
    await page.keyboard.press('Control+a');
    await expect(page.getByTestId('selection-bar')).toContainText('600 selected');
    await page.keyboard.press('Escape');
    await expect(page.getByTestId('selection-bar')).toHaveCount(0);
  });

  test('draws a selection band', async ({ page }) => {
    const grid = page.getByRole('listbox', { name: 'Photos' });
    const box = (await grid.boundingBox())!;
    await page.mouse.move(box.x + 30, box.y + 100);
    await page.mouse.down();
    await page.mouse.move(box.x + 700, box.y + 400, { steps: 8 });
    await page.mouse.up();
    await expect(page.getByTestId('selection-bar')).toBeVisible();
  });

  test('opens the viewer, moves and closes', async ({ page }) => {
    await page.locator('.tile:not(.hidden)').first().dblclick();
    const viewer = page.getByRole('dialog');
    await expect(viewer).toBeVisible();
    const title = await viewer.locator('.title').textContent();
    await page.keyboard.press('ArrowRight');
    await expect(viewer.locator('.title')).not.toHaveText(title ?? '');
    await page.keyboard.press('i');
    await expect(viewer.getByRole('complementary', { name: 'Info' })).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(viewer).toHaveCount(0);
  });

  test('quick look opens with Space', async ({ page }) => {
    await page.locator('.tile:not(.hidden)').first().click();
    await page.keyboard.press(' ');
    await expect(page.locator('.viewer.quick')).toBeVisible();
    await page.keyboard.press(' ');
    await expect(page.locator('.viewer')).toHaveCount(0);
  });

  test('filters by facet and shows a chip', async ({ page }) => {
    await page.getByTestId('filters-toggle').click();
    const filters = page.getByRole('complementary', { name: 'Filters' });
    await filters.getByRole('button', { name: /^Videos/ }).click();
    await expect(page.getByRole('button', { name: 'Remove filter Videos' })).toBeVisible();
    const flags = await page.locator('.tile:not(.hidden) .badge.text').count();
    expect(flags).toBeGreaterThan(0);
    await page.getByRole('button', { name: 'Remove filter Videos' }).click();
    await expect(page.getByRole('button', { name: 'Remove filter Videos' })).toHaveCount(0);
  });

  test('changes the row height with the slider', async ({ page }) => {
    const first = page.locator('.tile:not(.hidden)').first();
    const before = (await first.boundingBox())!.height;
    await page.getByRole('slider', { name: 'Row height' }).fill('120');
    await expect.poll(async () => (await page.locator('.tile:not(.hidden)').first().boundingBox())!.height).toBeLessThan(before);
  });

  test('command palette runs commands', async ({ page }) => {
    await page.keyboard.press('Control+k');
    await page.getByRole('textbox', { name: 'Command' }).fill('settings');
    await page.keyboard.press('Enter');
    await expect(page.getByRole('heading', { name: 'Library', exact: true })).toBeVisible();
  });

  test('switches the theme in settings', async ({ page }) => {
    await page.getByTestId('nav-settings').click();
    await page.getByRole('button', { name: 'Appearance' }).click();
    await page.getByRole('radio', { name: 'Dark' }).click();
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
    await page.getByRole('radio', { name: 'Light' }).click();
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
  });

  test('browses folders', async ({ page }) => {
    await page.getByTestId('nav-folders').click();
    const tree = page.getByRole('navigation', { name: 'Folders' });
    await tree.getByRole('button', { name: /^2019/ }).click();
    await expect(page.locator('.bar .path')).toHaveText('2019');
    await expect(page.locator('.tile:not(.hidden)').first()).toBeVisible();
  });

  test('keyboard moves the focus', async ({ page }) => {
    await page.getByRole('listbox', { name: 'Photos' }).focus();
    await page.keyboard.press('ArrowRight');
    await page.keyboard.press('ArrowRight');
    await expect(page.getByTestId('selection-bar')).toContainText('1 selected');
    await expect(page.locator('.tile.focused')).toHaveCount(1);
  });
});
