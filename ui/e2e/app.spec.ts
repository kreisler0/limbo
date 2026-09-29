import { expect, test, type Page } from '@playwright/test';

declare global {
  interface Window {
    __limboMock: {
      emit: (event: string, payload: unknown) => void;
      tabs: { id: number; state: string; url: string }[];
      responses: { id: number; value: unknown }[];
      settings: () => { onboardingDone: boolean; theme: string };
      invoke: (cmd: string, args?: Record<string, unknown>) => unknown;
    };
  }
}

/** Browser-reserved combos (Ctrl+T, Ctrl+W…) never reach a headless page, so dispatch them. */
async function press(page: Page, combo: string) {
  const parts = combo.split('+');
  const k = parts.pop()!;
  await page.evaluate(
    (init) => (document.activeElement ?? document.body).dispatchEvent(new KeyboardEvent('keydown', init)),
    {
      key: k.length === 1 ? k.toLowerCase() : k,
      ctrlKey: parts.includes('Control'),
      shiftKey: parts.includes('Shift'),
      altKey: parts.includes('Alt'),
      bubbles: true,
      cancelable: true,
    },
  );
}

const tabs = (page: Page) => page.locator('[data-tab]');
const omnibox = (page: Page) => page.getByRole('combobox');

// Any uncaught error in the UI fails the test.
let errors: string[] = [];

test.beforeEach(async ({ page }) => {
  errors = [];
  page.on('pageerror', (e) => errors.push(e.message));
  await page.goto('/');
  await expect(tabs(page)).toHaveCount(3);
});

test.afterEach(() => {
  expect(errors).toEqual([]);
});

test('new tab, type, navigate', async ({ page }) => {
  await page.getByRole('button', { name: 'New tab' }).first().click();
  await expect(tabs(page)).toHaveCount(4);
  await expect(omnibox(page)).toBeFocused();
  await omnibox(page).fill('rust');
  await expect(page.getByRole('listbox')).toBeVisible();
  await page.keyboard.press('Enter');
  await expect(omnibox(page)).toHaveValue(/rust/);
  await expect(page.getByRole('listbox')).toHaveCount(0);
});

test('host shortcuts go to the host once (no echo loop)', async ({ page }) => {
  await press(page, 'Control+t');
  await expect(tabs(page)).toHaveCount(4);
  await press(page, 'Control+w');
  await expect(tabs(page)).toHaveCount(3);
});

test('command palette opens, filters and closes', async ({ page }) => {
  await press(page, 'Control+k');
  const input = page.getByRole('dialog', { name: 'Command palette' }).getByRole('textbox');
  await expect(input).toBeFocused();
  await input.fill('hacker');
  await expect(page.getByRole('option').first()).toContainText('Hacker News');
  await page.keyboard.press('Escape');
  await expect(page.getByRole('dialog', { name: 'Command palette' })).toHaveCount(0);
});

test('memory popover shows per-tab usage and sleeps tabs', async ({ page }) => {
  await press(page, 'Control+Shift+m');
  const pop = page.getByRole('dialog', { name: 'Memory saver' });
  await expect(pop).toBeVisible();
  await expect(pop).toContainText('Browser engine');
  await pop.getByRole('button', { name: /Sleep all/ }).click();
  await expect.poll(() => page.evaluate(() => window.__limboMock.tabs.filter((t) => t.state === 'suspended').length)).toBeGreaterThan(0);
});

test('page context menu answers the host', async ({ page }) => {
  await page.evaluate(() =>
    window.__limboMock.emit('context-menu', {
      menuId: 42,
      tabId: 1,
      x: 200,
      y: 150,
      items: [
        { id: 7, label: 'Copy link', kind: 'item', enabled: true, checked: false, shortcut: null },
        { id: 8, label: 'Reload', kind: 'item', enabled: true, checked: false, shortcut: 'Ctrl+R' },
      ],
    }),
  );
  await page.getByRole('menuitem', { name: /Copy link/ }).click();
  await expect.poll(() => page.evaluate(() => window.__limboMock.responses)).toContainEqual({ id: 42, value: { itemId: 7 } });
});

test('dismissed context menu is released', async ({ page }) => {
  await page.evaluate(() =>
    window.__limboMock.emit('context-menu', {
      menuId: 43,
      tabId: 1,
      x: 10,
      y: 10,
      items: [{ id: 1, label: 'Back', kind: 'item', enabled: true, checked: false, shortcut: null }],
    }),
  );
  await expect(page.getByRole('menuitem', { name: /Back/ })).toBeVisible();
  await page.keyboard.press('Escape');
  await expect.poll(() => page.evaluate(() => window.__limboMock.responses)).toContainEqual({ id: 43, value: { itemId: null } });
});

test('theme setting applies', async ({ page }) => {
  await page.getByRole('button', { name: 'Menu' }).click();
  await page.getByRole('menuitem', { name: /Settings/ }).click();
  await page.locator('nav[aria-label="Settings sections"] button', { hasText: 'Appearance' }).click();
  await page.getByRole('radio', { name: 'Dark' }).click();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
});

test('find bar reports matches', async ({ page }) => {
  await press(page, 'Control+f');
  const find = page.getByRole('searchbox', { name: 'Find in page' }).or(page.getByPlaceholder('Find in page'));
  await find.fill('limbo');
  await expect(page.getByRole('search')).toContainText(/of/);
});

test.describe('first run', () => {
  test('onboarding: theme, import, extension, default browser', async ({ page }) => {
    await page.goto('/?onboarding=1');
    await page.getByRole('button', { name: 'Get started' }).click();
    await expect(page.getByText('Import from Firefox')).toBeVisible();
    // Open Firefox tabs are imported by default (owner's choice).
    await expect(page.getByRole('checkbox', { name: 'Open tabs' })).toBeChecked();
    await page.getByRole('button', { name: 'Import', exact: true }).click();
    await expect(page.getByText('Imported from Firefox')).toBeVisible({ timeout: 8000 });
    await page.getByRole('button', { name: 'Continue' }).click();
    await expect(page.getByText('Block ads and trackers')).toBeVisible();
    await page.getByRole('button', { name: /Continue|Not now/ }).click();
    await page.getByRole('button', { name: /Not now|Done/ }).click();
    await expect.poll(() => page.evaluate(() => window.__limboMock.settings().onboardingDone)).toBe(true);
  });
});
