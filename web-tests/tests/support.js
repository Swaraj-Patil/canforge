import { expect } from '@playwright/test';

/** Open the page and wait until the example bus is decoded on screen. */
export async function openExample(page, path = '/') {
  await page.goto(path);
  await expect(page.locator('#message-list button')).toHaveCount(7);
  await expect(page.locator('#decoded table')).toBeVisible();
}

/** The row of the decoded values table for one signal. */
export function signalRow(page, name) {
  return page.locator('#decoded tbody tr').filter({ has: page.locator('td:first-child', { hasText: new RegExp(`^${name}$`) }) });
}

export function median(values) {
  const sorted = [...values].sort((a, b) => a - b);
  const mid = sorted.length >> 1;
  return sorted.length % 2 ? sorted[mid] : (sorted[mid - 1] + sorted[mid]) / 2;
}
