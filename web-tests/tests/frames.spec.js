import { expect, openExample, signalRow, test } from './support.js';

test.beforeEach(async ({ page }) => {
  await openExample(page);
});

test('the example bus shows 7 messages', async ({ page }) => {
  await expect(page.locator('#message-list .name')).toHaveText([
    'VehicleStatus',
    'BatteryStatus',
    'InverterTelemetry',
    'ChargerLimits',
    'ThermalSensors',
    'WheelSpeeds',
    'DiagnosticFD',
  ]);
  await expect(page.locator('#db-summary')).toContainText('7 messages, 40 signals, 5 nodes');
});

test('flipping byte 0, bit 0 of VehicleStatus changes VehicleSpeed by 0.01 km/h', async ({ page }) => {
  await expect(page.locator('#message-detail h2')).toHaveText('VehicleStatus');
  const speed = signalRow(page, 'VehicleSpeed');
  await expect(speed.locator('td').nth(1)).toHaveText('10.00 km/h');
  await expect(speed.locator('td').nth(2)).toHaveText('1000');

  await page.getByRole('button', { name: /^Byte 0, bit 0 is 0,/ }).click();

  await expect(speed.locator('td').nth(1)).toHaveText('10.01 km/h');
  await expect(speed.locator('td').nth(2)).toHaveText('1001');
  await expect(page.locator('#hex-input')).toHaveValue('e9 03 03 5a 18 fc 00 05');
});

test('the summary says how long the file took to parse', async ({ page }) => {
  await expect(page.locator('#db-summary p')).toHaveText(/^7 messages, 40 signals, 5 nodes, parsed in (under 1|\d+) ms$/);
});

test('a file that does not parse leaves the open file working', async ({ page }) => {
  await page.setInputFiles('#file-input', { name: 'broken.dbc', mimeType: 'text/plain', buffer: Buffer.from('BO_ 1 M 8 A\n') });
  await expect(page.locator('#status')).toHaveText("broken.dbc could not be read (line 1): expected ':', found '8'.");
  await expect(page.locator('#db-summary h2')).toHaveText('powertrain.dbc');
  // The example is still the loaded database, so decoding keeps working.
  await page.getByRole('button', { name: /^Byte 0, bit 0 is 0,/ }).click();
  await expect(signalRow(page, 'VehicleSpeed').locator('td').nth(1)).toHaveText('10.01 km/h');
});

test('choosing a message shows its frame', async ({ page }) => {
  await page.locator('#message-list button', { hasText: 'WheelSpeeds' }).click();
  await expect(page.locator('#message-detail h2')).toHaveText('WheelSpeeds');
  await expect(page.locator('#message-detail .facts')).toHaveText('Frame 0x600, standard 11-bit ID, 8 bytes, sent by GATEWAY.');
  await expect(signalRow(page, 'YawRate').locator('td').nth(1)).toHaveText('1.50 deg/s');
});
