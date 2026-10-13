import { expect, type Page, test } from '@playwright/test';

async function aiMode(page: Page, mode: 'Local' | 'OpenRouter') {
  await page.getByTestId('nav-settings').click();
  await page.getByRole('button', { name: 'AI', exact: true }).click();
  await page.getByRole('radiogroup', { name: 'AI mode' }).getByRole('radio', { name: mode }).click();
}

test('a cloud run shows the estimate and consent first, then records its proposals', async ({ page }) => {
  await page.goto('/?setup=1&mock=800');
  await aiMode(page, 'OpenRouter');
  await page.getByPlaceholder('sk-or-…').fill('sk-or-test');
  await page.getByRole('button', { name: 'Save', exact: true }).click();
  await page.getByRole('button', { name: 'Network', exact: true }).click();
  await page.getByRole('switch', { name: 'AI provider' }).click();

  await page.getByTestId('nav-ai').click();
  const offers = page.getByTestId('ai-offers');
  // Highest quality first; providers that keep data are hidden.
  await expect(offers.locator('tbody tr').first()).toContainText('gemini-2.5-flash');
  await expect(page.getByTestId('best-cloud')).toContainText('Highest quality: Gemini 2.5 Flash');
  await expect(offers).not.toContainText('may keep data');
  await offers.locator('tr', { hasText: 'google/gemma-3-12b-it' }).getByRole('button', { name: 'Use' }).click();

  const consent = page.getByTestId('ai-consent');
  await expect(consent).toContainText(/thumbnails \(512 px, no metadata\) will be sent to DeepInfra through OpenRouter\. Estimated cost \$\d/);
  await page.getByTestId('ai-start').click();
  await expect(page.getByTestId('ai-runs')).toContainText('Done', { timeout: 15_000 });
  await expect(page.getByTestId('ai-runs')).toContainText('proposed');
  await expect(page.getByTestId('ai-run')).toContainText('already answered by this model');
});

test('the budget stops a run, and it continues with a higher one', async ({ page }) => {
  await page.goto('/?setup=1&mock=800');
  await aiMode(page, 'OpenRouter');
  await page.getByPlaceholder('sk-or-…').fill('sk-or-test');
  await page.getByRole('button', { name: 'Save', exact: true }).click();
  await page.getByRole('button', { name: 'Network', exact: true }).click();
  await page.getByRole('switch', { name: 'AI provider' }).click();
  await page.getByTestId('nav-ai').click();
  await page.getByTestId('ai-offers').locator('tr', { hasText: 'gemini-2.5-flash-lite' }).getByRole('button', { name: 'Use' }).click();
  const budget = page.getByTestId('ai-run').getByRole('spinbutton');
  await budget.fill('0.002');
  await budget.blur();
  await page.getByTestId('ai-start').click();
  const runs = page.getByTestId('ai-runs');
  await expect(runs).toContainText('Stopped at the budget', { timeout: 15_000 });
  await budget.fill('5');
  await budget.blur();
  await runs.getByRole('button', { name: /Continue/ }).click();
  await expect(runs).toContainText('Done', { timeout: 15_000 });
});

test('the model check compares models on labelled items', async ({ page }) => {
  await page.goto('/?setup=1&mock=800');
  await aiMode(page, 'Local');
  await page.getByTestId('nav-ai').click();
  await page.getByTestId('local-models').locator('tr', { hasText: 'qwen3-vl:4b-instruct' }).getByRole('button', { name: 'Use' }).click();
  await expect(page.getByTestId('ai-consent')).toContainText('Nothing leaves the machine');

  await page.getByTestId('check-sample').click();
  await expect(page.getByTestId('labeler')).toBeVisible();
  for (let i = 0; i < 24; i++) await page.keyboard.press(i % 3 === 0 ? '1' : 'Enter');
  await expect(page.getByTestId('labeler')).toContainText('24 labelled');

  const pick = page.getByRole('group', { name: 'Models to compare' });
  await pick.getByRole('button', { name: 'qwen3-vl:4b-instruct' }).click();
  await pick.getByRole('button', { name: 'gemma4:e4b' }).click();
  await page.getByTestId('check-run').click();
  const report = page.getByTestId('check-report');
  await expect(report.locator('tbody tr')).toHaveCount(2, { timeout: 15_000 });
  await expect(report).toContainText('Recommended');
});

test('search words the parser leaves can go to the text model', async ({ page }) => {
  await page.goto('/?setup=1&mock=300&ai=local');
  await page.getByTestId('nav-ai').click();
  await page.getByTestId('local-models').locator('tr', { hasText: 'qwen3-vl:4b-instruct' }).getByRole('button', { name: 'Use' }).click();
  const search = page.getByTestId('search');
  await search.fill('funny clips from christmas cat');
  await search.press('Enter');
  await page.getByTestId('ask-ai').click();
  await expect(page.getByRole('button', { name: 'Remove filter #memes' })).toBeVisible();
  await expect(search).toHaveValue('cat');
  await expect(page.getByTestId('ask-ai')).toBeHidden();
});
