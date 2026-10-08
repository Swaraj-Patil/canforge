import { expect, openExample, repoFile, test } from './support.js';

test.use({ permissions: ['clipboard-read', 'clipboard-write'] });

const hash = (page) => page.evaluate(() => location.hash);

test('a copied link restores the same frame', async ({ page, browser }) => {
  await openExample(page);
  await page.locator('#message-list button', { hasText: 'WheelSpeeds' }).click();
  await page.locator('#hex-input').fill('12 34 56 78 9a bc de f0');
  await page.locator('#matrix .bit[data-k="0"]').click();
  const values = await page.locator('#decoded tbody tr').allTextContents();

  await page.getByRole('button', { name: 'Copy link' }).click();
  await expect(page.getByRole('button', { name: 'Link copied' })).toBeVisible();
  const url = await page.evaluate(() => navigator.clipboard.readText());
  expect(url).toMatch(/\/#t=frames&m=WheelSpeeds&b=133456789abcdef0$/);

  // Someone else opens the link in a browser of their own.
  const other = await browser.newContext();
  const theirs = await other.newPage();
  await theirs.goto(url);
  await expect(theirs.locator('#message-detail h2')).toHaveText('WheelSpeeds');
  await expect(theirs.locator('#hex-input')).toHaveValue('13 34 56 78 9a bc de f0');
  await expect(theirs.locator('#decoded tbody tr')).toHaveText(values);
  await expect(theirs.getByRole('tab', { name: 'Frames' })).toHaveAttribute('aria-selected', 'true');
  await other.close();
});

test('the address bar follows the view of the example bus', async ({ page }) => {
  await openExample(page);
  await page.locator('#message-list button', { hasText: 'BatteryStatus' }).click();
  await page.getByRole('tab', { name: /^Problems/ }).click();
  await expect.poll(() => hash(page)).toBe('#t=problems&m=BatteryStatus&b=9c40ff06a01fb800');

  await page.reload();
  await expect(page.getByRole('tab', { name: /^Problems/ })).toHaveAttribute('aria-selected', 'true');
  await expect(page.locator('#view-problems')).toBeVisible();
  await page.getByRole('tab', { name: 'Frames' }).click();
  await expect(page.locator('#message-detail h2')).toHaveText('BatteryStatus');
});

test('the example keeps a plain address until the view changes', async ({ page }) => {
  await openExample(page);
  await page.waitForTimeout(400);
  expect(await hash(page)).toBe('');
  await page.locator('#matrix .bit[data-k="0"]').click();
  await expect.poll(() => hash(page)).toBe('#t=frames&m=VehicleStatus&b=e903035a18fc0005');
  // Flipping the bit back returns to the opening view and the plain address.
  await page.locator('#matrix .bit[data-k="0"]').click();
  await expect.poll(() => hash(page)).toBe('');
});

test('a flip made just before a reload survives it', async ({ page }) => {
  await openExample(page);
  await page.locator('#matrix .bit[data-k="0"]').click();
  await page.reload();
  await expect(page.locator('#hex-input')).toHaveValue('e9 03 03 5a 18 fc 00 05');
});

test('a burst of flips stays within the browser limit on address rewrites, and the last view lands', async ({ page }) => {
  // Safari throws after 100 rewrites in 30 seconds; count them.
  await page.addInitScript(() => {
    const replace = history.replaceState.bind(history);
    window.rewrites = 0;
    history.replaceState = (...args) => {
      window.rewrites += 1;
      return replace(...args);
    };
  });
  await openExample(page);
  await page.evaluate(() => {
    for (let i = 0; i < 60; i += 1) document.querySelector(`#matrix .bit[data-k="${i % 8}"]`).click();
  });
  // Of 60 flips, bits 0 to 3 of byte 0 get 8 each and bits 4 to 7 get 7, so 0xe8 becomes 0x18.
  await expect.poll(() => hash(page), { timeout: 5000 }).toBe('#t=frames&m=VehicleStatus&b=1803035a18fc0005');
  const rewrites = await page.evaluate(() => window.rewrites);
  expect(rewrites).toBeLessThanOrEqual(12);
});

test('a link pasted into the open page shows its view', async ({ page }) => {
  await openExample(page);
  await page.evaluate(() => {
    location.hash = '#t=frames&m=WheelSpeeds&b=0000000000000000';
  });
  await expect(page.locator('#message-detail h2')).toHaveText('WheelSpeeds');
  await expect(page.locator('#hex-input')).toHaveValue('00 00 00 00 00 00 00 00');
});

test('a link that asks for what the example lacks says what it left out', async ({ page }) => {
  await page.goto('/#t=frames&m=Nope&b=zz');
  await expect(page.locator('#message-list button')).toHaveCount(7);
  await expect(page.locator('#status')).toHaveText(
    "The link names a message the example bus does not have, Nope, so the first message is shown. " +
      `The link's bytes were left out: "zz" is not hexadecimal.`,
  );
  await expect(page.locator('#message-detail h2')).toHaveText('VehicleStatus');
});

test('your own files have no link', async ({ page }) => {
  await openExample(page);
  await expect(page.getByRole('button', { name: 'Copy link' })).toBeVisible();
  await page.setInputFiles('#file-input', repoFile('tests/fixtures/diff/v1.dbc'));
  await expect(page.locator('#db-summary h2')).toHaveText('v1.dbc');
  await expect(page.getByRole('button', { name: 'Copy link' })).toHaveCount(0);
  await expect.poll(() => hash(page)).toBe('');
});
