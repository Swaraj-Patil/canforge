import { expect, openExample, repoFile, test } from './support.js';

const note = (page) => page.locator('#storage-note');
const title = (page) => page.locator('#db-summary h2');

test('a file you open opens again next time, until you forget it', async ({ page }) => {
  await openExample(page);
  await expect(note(page)).toBeHidden();
  await page.setInputFiles('#file-input', repoFile('tests/fixtures/diff/v2.dbc'));
  await expect(title(page)).toHaveText('v2.dbc');
  await expect(note(page)).toContainText('This browser keeps a copy so the file opens next time. The copy never leaves your computer.');

  await page.reload();
  await expect(title(page)).toHaveText('v2.dbc');
  await expect(note(page)).toContainText('This browser keeps a copy');

  await note(page).getByRole('button', { name: 'Forget this file' }).click();
  await expect(note(page)).toHaveText('This browser no longer keeps a copy of the file.');
  expect(await page.evaluate(() => localStorage.length)).toBe(0);
  await page.reload();
  await expect(title(page)).toHaveText('powertrain.dbc');
});

test('the example names the file this browser keeps, and opens it', async ({ page }) => {
  await openExample(page);
  await page.setInputFiles('#file-input', repoFile('tests/fixtures/diff/v2.dbc'));
  await expect(title(page)).toHaveText('v2.dbc');
  await page.getByRole('button', { name: 'Use the example bus' }).click();
  await expect(title(page)).toHaveText('powertrain.dbc');
  await expect(note(page)).toContainText('This browser keeps your last file, v2.dbc, and opens it next time.');
  await note(page).getByRole('button', { name: 'Open it' }).click();
  await expect(title(page)).toHaveText('v2.dbc');
});

test('a file over 2 MB is not kept, and the page says so', async ({ page }) => {
  await openExample(page);
  // A valid database padded past 2 MB with a long comment.
  const text = `BO_ 256 Big: 8 A\n SG_ S : 0|8@1+ (1,0) [0|255] "" A\nCM_ "${'x'.repeat(2 * 1024 * 1024)}";\n`;
  await page.setInputFiles('#file-input', { name: 'big.dbc', mimeType: 'text/plain', buffer: Buffer.from(text) });
  await expect(title(page)).toHaveText('big.dbc');
  await expect(note(page)).toHaveText('canforge keeps files of up to 2 MB, so this one will not open next time.');
  await page.reload();
  await expect(title(page)).toHaveText('powertrain.dbc');
});

test('a kept file that no longer opens is removed, and the page says why', async ({ page }) => {
  await openExample(page);
  await page.evaluate(() => localStorage.setItem('canforge.lastFile', JSON.stringify({ name: 'old.dbc', text: 'BO_ 1 M 8 A\n' })));
  await page.reload();
  await expect(title(page)).toHaveText('powertrain.dbc');
  await expect(page.locator('#status')).toHaveText(
    "old.dbc could not be read (line 1): expected ':', found '8'. canforge removed the copy this browser kept.",
  );
  expect(await page.evaluate(() => localStorage.getItem('canforge.lastFile'))).toBeNull();
});
