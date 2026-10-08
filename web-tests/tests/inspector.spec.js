import { expect, openExample, repoFile, signalRow, test } from './support.js';

test.beforeEach(async ({ page }) => {
  await openExample(page);
});

const panel = (page) => page.locator('#inspector');
// The text of one row of the inspector's facts.
const fact = (page, term) => panel(page).locator('dt', { hasText: term }).locator('xpath=following-sibling::dd[1]');

async function inspect(page, message, signal) {
  await page.locator('#message-list button', { hasText: message }).click();
  await expect(page.locator('#message-detail h2')).toHaveText(message);
  await signalRow(page, signal).getByRole('button').click();
  await expect(panel(page).locator('h3')).toHaveText(signal);
}

test('a Motorola signal: layout in words, ranges, receivers and its generated C', async ({ page }) => {
  await inspect(page, 'WheelSpeeds', 'WheelSpeedFL');
  await expect(fact(page, 'Layout')).toContainText('Motorola, 12 bits, starting at bit 7, most significant bit first');
  await expect(fact(page, 'Layout')).toContainText('Byte 0 bits 7 to 0, byte 1 bits 7 to 4. In the file: 7|12@0+');
  await expect(fact(page, 'Type')).toHaveText('Unsigned integer');
  await expect(fact(page, 'Scaling')).toHaveText('physical = raw × 0.1');
  await expect(fact(page, 'Declared range')).toHaveText('0 to 409.5 km/h');
  await expect(fact(page, 'Range the bits can hold')).toHaveText('0 to 409.5 km/h (raw 0 to 4095)');
  await expect(fact(page, 'Receivers')).toHaveText('VCU');

  const code = panel(page).locator('figure');
  await expect(code.locator('figcaption')).toHaveText([
    'Struct member in powertrain.h',
    'In powertrain_wheel_speeds_pack()',
    'In powertrain_wheel_speeds_unpack()',
    'Decode, encode and range check in powertrain.c',
  ]);
  await expect(code.nth(0).locator('pre')).toContainText('uint16_t wheel_speed_fl;');
  await expect(code.nth(1).locator('pre')).toContainText('dst_p[0] = (uint8_t)(dst_p[0] | (uint8_t)((v >> 4) & 0xFFu));');
  await expect(code.nth(3).locator('pre')).toContainText('double powertrain_wheel_speeds_wheel_speed_fl_decode(uint16_t raw)');
});

test('an Intel signal with a comment, and the row it was opened from', async ({ page }) => {
  await signalRow(page, 'VehicleSpeed').click();
  await expect(fact(page, 'Layout')).toContainText('Intel, 16 bits, starting at bit 0, least significant bit first');
  await expect(panel(page).locator('.comment')).toHaveText('Vehicle speed derived from wheel speeds.');
  await expect(signalRow(page, 'VehicleSpeed')).toHaveClass(/\binspected\b/);
  await expect(signalRow(page, 'VehicleSpeed').getByRole('button')).toHaveAttribute('aria-expanded', 'true');
});

test('value descriptions, signed ranges and multiplexer pages', async ({ page }) => {
  await signalRow(page, 'GearPosition').click();
  await expect(fact(page, 'Scaling')).toHaveText('physical = raw (no scaling)');
  await expect(fact(page, 'Value descriptions').locator('tr')).toHaveText(['0Park', '1Reverse', '2Neutral', '3Drive', '7Invalid']);

  await signalRow(page, 'SteeringAngle').click();
  await expect(fact(page, 'Type')).toHaveText("Signed integer (two's complement)");
  await expect(fact(page, 'Range the bits can hold')).toHaveText('−3276.8 to 3276.7 deg (raw −32768 to 32767)');

  await inspect(page, 'InverterTelemetry', 'PageIndex');
  await expect(fact(page, 'Multiplexing')).toHaveText('This is the multiplexer: its value selects which signals the frame carries.');
  await page.getByRole('button', { name: '1: Temperatures' }).click();
  await signalRow(page, 'StatorTemp').click();
  await expect(fact(page, 'Multiplexing')).toHaveText('Present when PageIndex is 1 (Temperatures).');
  await expect(fact(page, 'Scaling')).toHaveText('physical = raw − 40');
  await expect(panel(page).locator('figure').nth(1).locator('pre')).toContainText('        /* StatorTemp */');
});

test('a float signal and a 64-bit signal', async ({ page }) => {
  await inspect(page, 'ThermalSensors', 'CoolantFlow');
  await expect(fact(page, 'Type')).toHaveText('IEEE 754 single-precision float');
  await expect(fact(page, 'Range the bits can hold')).toHaveText(
    '−3.402823e+38 to 3.402823e+38 L/min (any finite float32 value)',
  );
  await inspect(page, 'DiagnosticFD', 'SerialNumber');
  await expect(fact(page, 'Declared range')).toHaveText('Not specified (the file gives [0|0])');
  // Decoding the all-ones raw value gives the double 2^64, which is shown in full.
  await expect(fact(page, 'Range the bits can hold')).toHaveText('0 to 18446744073709551616 (raw 0 to 18446744073709551615)');
});

test('the inspector uses the name prefix from Generated code', async ({ page }) => {
  await page.getByRole('tab', { name: 'Generated code' }).click();
  await page.getByLabel('Name prefix').fill('ecu');
  await page.getByRole('tab', { name: 'Frames' }).click();
  await signalRow(page, 'VehicleSpeed').click();
  await expect(panel(page).locator('figcaption').nth(1)).toHaveText('In ecu_vehicle_status_pack()');
  await expect(panel(page).locator('figure').nth(3).locator('pre')).toContainText('double ecu_vehicle_status_vehicle_speed_decode(uint16_t raw)');
});

test('Close and Esc close the inspector and return focus to the signal', async ({ page }) => {
  await signalRow(page, 'VehicleSpeed').getByRole('button').click();
  await panel(page).getByRole('button', { name: 'Close' }).click();
  await expect(panel(page)).toBeHidden();
  await expect(signalRow(page, 'VehicleSpeed').getByRole('button')).toBeFocused();
  await expect(signalRow(page, 'VehicleSpeed').getByRole('button')).toHaveAttribute('aria-expanded', 'false');

  await page.keyboard.press('Enter');
  await expect(panel(page)).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(panel(page)).toBeHidden();
  await expect(signalRow(page, 'VehicleSpeed').getByRole('button')).toBeFocused();
});

test('the inspected signal stays lit in the grid while nothing else is pointed at', async ({ page }) => {
  await signalRow(page, 'SteeringAngle').click();
  await page.mouse.move(2, 2);
  await expect(page.locator('#matrix .bit[data-k="0"]')).toHaveClass(/\bdim\b/);
  await expect(page.locator('#matrix .bit[data-k="32"]')).not.toHaveClass(/\bdim\b/);
  // A flip keeps it lit.
  await page.locator('#matrix .bit[data-k="0"]').click();
  await page.mouse.move(2, 2);
  await expect(page.locator('#matrix .bit[data-k="0"]')).toHaveClass(/\bdim\b/);
  await expect(panel(page).locator('h3')).toHaveText('SteeringAngle');
});

test('a search hit found through a signal opens that signal', async ({ page }) => {
  await page.getByLabel('Find a message').fill('yaw');
  await page.locator('#message-list li:not([hidden]) button').click();
  await expect(page.locator('#message-detail h2')).toHaveText('WheelSpeeds');
  await expect(panel(page).locator('h3')).toHaveText('YawRate');
});

test('a file with lint errors explains why there is no C', async ({ page }) => {
  await page.setInputFiles('#file-input', repoFile('tests/fixtures/lint/e001_signal_overlap.dbc'));
  await expect(page.locator('#db-summary h2')).toHaveText('e001_signal_overlap.dbc');
  await signalRow(page, 'Y').click();
  await expect(fact(page, 'Problems')).toContainText("Error E001: Signals 'X' and 'Y' in message 'Msg' overlap at bit 4, 5, 6, 7.");
  await expect(panel(page)).toContainText('Code generation is off while the file has errors. The Problems tab lists them.');
  await expect(panel(page).locator('figure')).toHaveCount(0);
});
