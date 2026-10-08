// Timing budgets from docs/web-roadmap.md: a bit flip updates the page within
// 16 ms on the example bus and within 50 ms on a 5,000-signal database.
//
// A flip is timed in the page, from the click until the browser has laid out
// the result, and the median of 20 flips is held to the budget. Medians are
// printed on every run, so a regression shows even when it stays inside the
// budget. With PERF_BUDGETS=report a miss is printed instead of failing.
import { fileURLToPath } from 'node:url';
import { expect, median, openExample, test } from './support.js';

const LARGE = fileURLToPath(new URL('../../build/large.dbc', import.meta.url));
const FLIPS = 20;

async function timeFlips(page) {
  return page.evaluate(async (count) => {
    const times = [];
    for (let i = 0; i < count; i += 1) {
      // Start every flip from a painted frame.
      await new Promise((resolve) => requestAnimationFrame(() => setTimeout(resolve, 0)));
      const bit = document.querySelector('#matrix .bit[data-k="0"]');
      const start = performance.now();
      bit.click();
      document.body.getBoundingClientRect(); // forces style and layout
      times.push(performance.now() - start);
    }
    return times;
  }, FLIPS);
}

function holdToBudget(what, times, budget) {
  const m = median(times);
  const slowest = Math.max(...times);
  console.log(
    `${what}: median ${m.toFixed(2)} ms over ${times.length} flips, ` +
      `${m <= budget ? 'within' : 'OVER'} the ${budget} ms budget (slowest ${slowest.toFixed(2)} ms)`,
  );
  if (process.env.PERF_BUDGETS !== 'report') {
    expect(m, `${what}: median of ${times.length} flips`).toBeLessThanOrEqual(budget);
  }
}

async function select(page, button) {
  const name = (await button.locator('.name').textContent()).trim();
  await button.click();
  await expect(page.locator('#message-detail h2')).toHaveText(name);
  return name;
}

test('a bit flip updates the page within 16 ms on the example bus', async ({ page }) => {
  await openExample(page);
  for (const name of ['VehicleStatus', 'DiagnosticFD']) {
    await select(page, page.locator('#message-list button', { hasText: name }));
    holdToBudget(`Example bus, ${name}`, await timeFlips(page), 16);
  }
});

test('a bit flip updates the page within 50 ms on a 5,000-signal database', async ({ page }) => {
  await openExample(page);
  const start = Date.now();
  await page.setInputFiles('#file-input', LARGE);
  await expect(page.locator('#db-summary h2')).toHaveText('large.dbc');
  await expect(page.locator('#message-list button')).toHaveCount(500);
  console.log(`large.dbc: opened and listed in ${Date.now() - start} ms, timed from the test (includes Playwright overhead)`);
  console.log(`large.dbc: the summary reads "${await page.locator('#db-summary p').textContent()}"`);

  const messages = page.locator('#message-list button');
  for (const size of [', 8 bytes', ', 64 bytes']) {
    const name = await select(page, messages.filter({ hasText: size }).first());
    holdToBudget(`5,000-signal database, ${name}${size}`, await timeFlips(page), 50);
  }
});
