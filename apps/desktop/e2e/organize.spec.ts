import { expect, test, type Page } from '@playwright/test';

const tiles = (page: Page) => page.locator('.tile:not(.hidden)');
const count = (page: Page, testid: string) => page.getByTestId(testid).locator('.count');

test.describe('edits, trash and history', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/?setup=1&mock=600');
    await expect(tiles(page).first()).toBeVisible();
  });

  test('moves items to the trash, restores with Ctrl+Z', async ({ page }) => {
    await tiles(page).nth(0).click();
    await tiles(page).nth(1).click({ modifiers: ['Control'] });
    await page.keyboard.press('Delete');
    await expect(page.getByTestId('toast')).toContainText('Moved 2 items to the trash');
    await expect(count(page, 'nav-trash')).toHaveText('2');
    await page.getByTestId('nav-trash').click();
    await expect(tiles(page)).toHaveCount(2);
    await page.keyboard.press('Control+z');
    await expect(page.getByTestId('toast')).toContainText('Undoing');
    await expect(page.getByText('The trash is empty.')).toBeVisible();
  });

  test('sets a category from the top bar and undoes it in History', async ({ page }) => {
    await tiles(page).nth(0).click();
    await page.getByTestId('set-category').click();
    await page.getByRole('menuitem', { name: 'Wallpapers' }).click();
    await expect(page.getByTestId('toast')).toContainText(/Moved 1 item to Wallpapers|Already in Wallpapers/);
    await page.getByTestId('nav-history').click();
    const history = page.getByTestId('history');
    await expect(history.locator('li').first()).toContainText('to Wallpapers');
    await history.getByRole('button', { name: /^Undo/ }).first().click();
    await expect(history.locator('li').first()).toContainText('Undone');
  });

  test('tags with T and removes the tag in the info panel', async ({ page }) => {
    await tiles(page).nth(0).click();
    await page.keyboard.press('t');
    await page.getByTestId('tag-input').fill('Trips/Lake Garda');
    await page.keyboard.press('Enter');
    await expect(page.getByTestId('toast')).toContainText('Tagged 1 item Trips/Lake Garda');
    await page.keyboard.press('i');
    const info = page.getByRole('complementary', { name: 'Info' });
    await expect(info.getByText('Trips/Lake Garda')).toBeVisible();
    await info.getByRole('button', { name: 'Remove tag Trips/Lake Garda' }).click();
    await expect(info.getByText('Trips/Lake Garda')).toHaveCount(0);
  });

  test('rates and favorites in the viewer', async ({ page }) => {
    await tiles(page).first().dblclick();
    const viewer = page.getByRole('dialog');
    await expect(viewer).toBeVisible();
    await page.keyboard.press('4');
    await expect(viewer.getByRole('radio', { name: /^4 of 5/ })).toHaveAttribute('aria-checked', 'true');
    await page.keyboard.press('f');
    await expect(page.getByTestId('toast')).toContainText(/favorites/);
  });

  test('sends trashed items to the Recycle Bin after confirming', async ({ page }) => {
    await tiles(page).nth(0).click();
    await page.keyboard.press('Delete');
    await page.getByTestId('nav-trash').click();
    await page.getByTestId('empty-trash').click();
    await page.getByTestId('confirm-recycle').click();
    await expect(page.getByTestId('toast')).toContainText('Sent 1 file to the Recycle Bin');
    await expect(page.getByText('The trash is empty.')).toBeVisible();
  });
});

test.describe('organize', () => {
  test('previews a plan and applies it', async ({ page }) => {
    await page.goto('/?setup=1&mock=300');
    await expect(tiles(page).first()).toBeVisible();
    await page.getByTestId('nav-organize').click();
    await page.getByTestId('preview-plan').click();
    const plan = page.getByTestId('plan');
    await expect(plan.getByText('Target folders')).toBeVisible();
    await expect(plan.locator('.folders li').first()).toBeVisible();
    await page.getByTestId('apply-plan').click();
    await expect(page.getByTestId('toast')).toContainText('Applying the plan');
    await page.getByTestId('preview-plan').click();
    await expect(page.getByTestId('apply-plan')).toHaveText(/Nothing to change/);
  });

  test('adds a rule in Settings', async ({ page }) => {
    await page.goto('/?setup=1&mock=100');
    await page.getByTestId('nav-settings').click();
    await page.getByRole('button', { name: 'Rules', exact: true }).click();
    await page.getByRole('button', { name: 'Add rule' }).click();
    await page.getByLabel('Rule name').fill('Scanner receipts');
    await page.getByLabel('Value').fill('^SCAN_');
    await page.getByLabel('Category').selectOption('documents');
    await page.getByRole('button', { name: 'Save rule' }).click();
    await expect(page.getByTestId('rules')).toContainText('Scanner receipts');
    await expect(page.getByTestId('rules')).toContainText('File name matches the pattern ^SCAN_ → Documents');
  });
});
