import { expect, test, type Page } from '@playwright/test';

const tiles = (page: Page) => page.locator('.tile:not(.hidden)');

test.beforeEach(async ({ page }) => {
  await page.goto('/?setup=1&mock=800');
  await expect(tiles(page).first()).toBeVisible();
});

test('the map shows clusters with thumbnails and a tray', async ({ page }) => {
  await page.getByTestId('nav-map').click();
  await expect(page.locator('.roll-pin').first()).toBeVisible({ timeout: 15_000 });
  await expect(page.getByTestId('tray').locator('.thumb').first()).toBeVisible();
  // Clicking a pin opens its items beside the map.
  await page.locator('.roll-pin').first().click();
  await expect(page.getByRole('complementary', { name: 'Items on the map' })).toBeVisible();
});

test('dropping tray items on the map sets their place', async ({ page }) => {
  await page.getByTestId('nav-map').click();
  await expect(page.locator('.roll-pin').first()).toBeVisible({ timeout: 15_000 });
  const total = async () => Number((await page.getByRole('region', { name: 'Items without a location' }).locator('header .mono').textContent())!.replace(/[^0-9]/g, ''));
  await expect.poll(total).toBeGreaterThan(0);
  const before = await total();
  const map = page.getByTestId('map');
  const box = (await map.boundingBox())!;
  await page.getByTestId('tray').locator('.thumb').first().evaluate((el, at) => {
    const dt = new DataTransfer();
    el.dispatchEvent(new DragEvent('dragstart', { dataTransfer: dt, bubbles: true }));
    const target = document.querySelector('[data-testid=map]')!;
    target.dispatchEvent(new DragEvent('dragover', { dataTransfer: dt, bubbles: true, clientX: at.x, clientY: at.y }));
    target.dispatchEvent(new DragEvent('drop', { dataTransfer: dt, bubbles: true, clientX: at.x, clientY: at.y }));
  }, { x: box.x + box.width / 2, y: box.y + box.height / 2 });
  await expect(page.getByTestId('toast')).toContainText('Set the location of 1 item');
  await expect.poll(total).toBe(before - 1);
});

test('accepts a place suggestion and offers the time zone', async ({ page }) => {
  await page.getByTestId('nav-places').click();
  const groups = page.getByTestId('place-groups');
  await expect(groups.locator('li').first()).toBeVisible();
  const first = await groups.locator('li').first().locator('strong').textContent();
  await groups.getByTestId('accept-place').first().click();
  await expect(page.getByTestId('toast')).toContainText('Set the location of');
  await expect(page.getByRole('dialog', { name: 'Set time zones too?' })).toBeVisible();
  await page.getByTestId('apply-zone').first().click();
  await expect(page.getByRole('dialog', { name: 'Set time zones too?' })).toBeHidden();
  await expect(groups.locator('li').first().locator('strong')).not.toHaveText(first ?? '');
});

test('sets a place by search with L, and pasted coordinates', async ({ page }) => {
  await tiles(page).first().click();
  await page.keyboard.press('l');
  await page.getByTestId('location-input').fill('riva');
  await expect(page.getByRole('option', { name: /Riva del Garda/ })).toBeVisible();
  await page.keyboard.press('Enter');
  await expect(page.getByTestId('toast')).toContainText('Riva del Garda');
  await tiles(page).nth(1).click();
  await page.keyboard.press('l');
  await page.getByTestId('location-input').fill('45.6789, 10.6543');
  await expect(page.getByRole('option', { name: /Coordinates/ })).toBeVisible();
  await page.keyboard.press('Enter');
  await expect(page.getByTestId('toast')).toContainText('Set the location of 1 item');
});

test('shifts a selection by an amount with D', async ({ page }) => {
  await tiles(page).first().click();
  await page.keyboard.press('d');
  await page.getByTestId('amount-hours').fill('2');
  await expect(page.getByTestId('date-preview')).toContainText('1 item');
  await page.getByTestId('apply-date').click();
  await expect(page.getByTestId('toast')).toContainText('Changed the date of 1 item');
});

test('fixes a camera set from a reference in a few steps', async ({ page }) => {
  // 1: open Fix dates; the suspect set opens by itself.
  await page.getByTestId('nav-dates').click();
  await expect(page.getByText('The folder says 2019')).toBeVisible();
  // 2: pick a reference photo.
  await page.getByTestId('set-thumb').first().click();
  // 3: enter its true time (a year earlier).
  const input = page.getByTestId('true-date');
  const value = await input.inputValue();
  await input.fill(`${Number(value.slice(0, 4)) - 1}${value.slice(4)}`);
  await expect(page.getByTestId('date-preview')).toContainText('items');
  // 4: apply.
  await page.getByTestId('apply-dates').click();
  await expect(page.getByTestId('toast')).toContainText('Fixed the date of');
});
