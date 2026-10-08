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

test('choosing a message shows its frame', async ({ page }) => {
  await page.locator('#message-list button', { hasText: 'WheelSpeeds' }).click();
  await expect(page.locator('#message-detail h2')).toHaveText('WheelSpeeds');
  await expect(page.locator('#message-detail .facts')).toHaveText('Frame 0x600, standard 11-bit ID, 8 bytes, sent by GATEWAY.');
  await expect(signalRow(page, 'YawRate').locator('td').nth(1)).toHaveText('1.50 deg/s');
});
