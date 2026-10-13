import AxeBuilder from '@axe-core/playwright';
import { expect, test } from '@playwright/test';

// Every main view, checked with axe for WCAG 2.1 A and AA problems.
const VIEWS = [
  'nav-timeline',
  'nav-folders',
  'nav-map',
  'nav-events',
  'nav-add',
  'nav-review',
  'nav-discover',
  'nav-duplicates',
  'nav-dates',
  'nav-places',
  'nav-organize',
  'nav-immich',
  'nav-history',
  'nav-trash',
  'nav-activity',
  'nav-settings',
];

for (const nav of VIEWS) {
  test(`no accessibility problems in ${nav.slice(4)}`, async ({ page }) => {
    await page.goto('/?setup=1&mock=300');
    await page.getByTestId(nav).click();
    await page.waitForTimeout(400);
    await page
      .locator('[role=listbox][aria-label=Photos] [role=option]')
      .first()
      .waitFor({ timeout: 2000 })
      .catch(() => undefined);
    const r = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa']).analyze();
    const found = r.violations.map((v) => `${v.id} (${v.impact}): ${v.help} — ${v.nodes.length} × ${v.nodes[0]?.target.join(' ')}`);
    expect(found).toEqual([]);
  });
}

test('no accessibility problems in first run', async ({ page }) => {
  await page.goto('/?mock=0');
  await page.waitForTimeout(400);
  const r = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa']).analyze();
  expect(r.violations.map((v) => `${v.id}: ${v.help} — ${v.nodes[0]?.target.join(' ')}`)).toEqual([]);
});

// Sort with AI with the labelling panel open, light and dark.
for (const scheme of ['light', 'dark'] as const) {
  test(`no accessibility problems in sort with AI, ${scheme}`, async ({ page }) => {
    await page.emulateMedia({ colorScheme: scheme });
    await page.goto('/?setup=1&mock=300&ai=local');
    await page.getByTestId('nav-ai').click();
    await page.getByTestId('local-models').locator('tr', { hasText: 'qwen3-vl:4b-instruct' }).getByRole('button', { name: 'Use' }).click();
    await page.getByTestId('check-sample').click();
    await page.getByTestId('labeler').waitFor();
    await page.waitForTimeout(300);
    const r = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa']).analyze();
    expect(r.violations.map((v) => `${v.id}: ${v.help} — ${v.nodes.length} × ${v.nodes[0]?.target.join(' ')}`)).toEqual([]);
  });
}

// The dark theme has its own colors: the same check on a few busy views.
for (const nav of ['nav-timeline', 'nav-add', 'nav-immich', 'nav-settings']) {
  test(`no accessibility problems in ${nav.slice(4)}, dark`, async ({ page }) => {
    await page.emulateMedia({ colorScheme: 'dark' });
    await page.goto('/?setup=1&mock=300');
    await page.getByTestId(nav).click();
    await page.waitForTimeout(400);
    const r = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa']).analyze();
    expect(r.violations.map((v) => `${v.id}: ${v.help} — ${v.nodes.length} × ${v.nodes[0]?.target.join(' ')}`)).toEqual([]);
  });
}
