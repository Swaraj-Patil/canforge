import { test, expect } from '@playwright/test';
import { openExample } from './support.js';

test.beforeEach(async ({ page }) => {
  await openExample(page);
});

test('Problems shows the I002 note', async ({ page }) => {
  await expect(page.locator('#problem-count')).toHaveText('1');
  await page.getByRole('tab', { name: /^Problems/ }).click();
  const items = page.locator('#problems li');
  await expect(items).toHaveCount(1);
  await expect(items.first().locator('.sev')).toHaveText('Note');
  await expect(items.first().locator('.rule')).toHaveText('I002 can-fd-frame');
  await expect(items.first()).toContainText("Message 'DiagnosticFD' is 64 bytes long, so it needs CAN FD.");
});

test('Generated code lists powertrain.h', async ({ page }) => {
  await page.getByRole('tab', { name: 'Generated code' }).click();
  await expect(page.locator('#code-files button')).toHaveText(['powertrain.h', 'powertrain.c']);
  await expect(page.locator('#code-output')).toContainText('#ifndef POWERTRAIN_H');
});

test('Compare with the example revisions reports breaking changes', async ({ page }) => {
  await page.getByRole('tab', { name: 'Compare revisions' }).click();
  await page.getByRole('button', { name: 'Use the example revisions' }).click();
  await expect(page.locator('#compare-files')).toHaveText('Old: v1.dbc. New: v2.dbc.');
  const verdict = page.locator('#compare-result .verdict');
  await expect(verdict).toHaveClass(/\bbreaking\b/);
  await expect(verdict).toHaveText('Breaking: existing decoders would misread some frames.');
  await expect(page.locator('#compare-result')).toContainText('4 breaking, 4 needing review, 5 compatible.');
  await expect(page.locator('#compare-result')).toContainText('Status.Counter layout changed: start bit 56 -> 60');
});
