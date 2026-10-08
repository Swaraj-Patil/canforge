import { expect, openExample, repoFile, test } from './support.js';

test.beforeEach(async ({ page }) => {
  await openExample(page);
});

const shown = (page) => page.locator('#message-list li:not([hidden]) .name');
const search = (page) => page.getByLabel('Find a message');

test('searching yaw finds WheelSpeeds and names the signal that matched', async ({ page }) => {
  await search(page).fill('yaw');
  await expect(shown(page)).toHaveText(['WheelSpeeds']);
  await expect(page.locator('#message-list li:not([hidden]) .found')).toHaveText('Signal YawRate');
  await expect(page.locator('#message-list li:not([hidden]) .found mark')).toHaveText('Yaw');
  await expect(page.locator('#search-count')).toHaveText('Showing 1 of 7 messages.');
});

test('a frame ID finds its message in hex or decimal', async ({ page }) => {
  const cases = [
    ['0x400', ['ThermalSensors']],
    ['1024', ['ThermalSensors']],
    ['0x0600', ['WheelSpeeds']],
    ['0x18FF50E5', ['ChargerLimits']],
    // Hex narrows while it is typed.
    ['0x1', ['VehicleStatus', 'ChargerLimits']],
    ['0x18', ['ChargerLimits']],
  ];
  for (const [query, names] of cases) {
    await search(page).fill(query);
    await expect(shown(page), query).toHaveText(names);
  }
  await search(page).fill('0x400');
  await expect(page.locator('#message-list li:not([hidden]) .id mark')).toHaveText('0x400');
});

test('a search says which signals matched', async ({ page }) => {
  await search(page).fill('speed');
  await expect(shown(page)).toHaveText(['VehicleStatus', 'InverterTelemetry', 'WheelSpeeds']);
  await expect(page.locator('#message-list li:not([hidden]) .found')).toHaveText([
    'Signal VehicleSpeed',
    'Signal MotorSpeed',
    'Signals WheelSpeedFL, WheelSpeedFR and 3 more',
  ]);
  await expect(page.locator('#message-list li:not([hidden]) .name mark')).toHaveText(['Speed']);
});

test('/ focuses the search from any tab, and Esc clears it', async ({ page }) => {
  await page.getByRole('tab', { name: /^Problems/ }).click();
  await page.keyboard.press('/');
  await expect(page.getByRole('tab', { name: 'Frames' })).toHaveAttribute('aria-selected', 'true');
  await expect(search(page)).toBeFocused();
  await page.keyboard.type('yaw');
  await expect(shown(page)).toHaveCount(1);

  await page.keyboard.press('Escape');
  await expect(search(page)).toHaveValue('');
  await expect(shown(page)).toHaveCount(7);
  await expect(page.locator('#search-count')).toBeHidden();
  await expect(page.locator('#message-list mark')).toHaveCount(0);
  await page.keyboard.press('Escape');
  await expect(search(page)).not.toBeFocused();
});

test('Enter opens the first match', async ({ page }) => {
  await search(page).fill('thermal');
  await page.keyboard.press('Enter');
  await expect(page.locator('#message-detail h2')).toHaveText('ThermalSensors');
  await expect(page.locator('#message-list button[aria-current="true"] .name')).toHaveText('ThermalSensors');
  await expect(search(page)).toBeFocused();
});

test('a search with no matches says so', async ({ page }) => {
  await search(page).fill('nothing here');
  await expect(shown(page)).toHaveCount(0);
  await expect(page.locator('#search-count')).toHaveText('No message, signal or frame ID matches "nothing here".');
});

test('a slash typed into another field stays in that field', async ({ page }) => {
  await page.locator('#hex-input').click();
  await page.keyboard.press('End');
  await page.keyboard.type('/');
  await expect(page.locator('#hex-input')).toHaveValue('e8 03 03 5a 18 fc 00 05/');
  await expect(search(page)).not.toBeFocused();
});

test('opening another file clears the search', async ({ page }) => {
  await search(page).fill('yaw');
  await page.setInputFiles('#file-input', repoFile('tests/fixtures/diff/v1.dbc'));
  await expect(page.locator('#db-summary h2')).toHaveText('v1.dbc');
  await expect(search(page)).toHaveValue('');
  await expect(page.locator('#message-list li[hidden]')).toHaveCount(0);
});
