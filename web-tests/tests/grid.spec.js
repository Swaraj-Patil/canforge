import { expect, openExample, signalRow, test } from './support.js';

test.beforeEach(async ({ page }) => {
  await openExample(page);
});

const hexOf = (page, byte) => page.locator('#matrix .byte-value:not(.colhead):not(.dec)').nth(byte);
const decOf = (page, byte) => page.locator('#matrix .byte-value.dec:not(.colhead)').nth(byte);
const names = (page) => page.locator('#matrix .run');

async function open(page, name) {
  await page.locator('#message-list button', { hasText: name }).click();
  await expect(page.locator('#message-detail h2')).toHaveText(name);
}

test('each byte row shows its value in hex and decimal, updating live', async ({ page }) => {
  // The hidden words read the value out to screen readers.
  await expect(hexOf(page, 0)).toHaveText('Byte 0 is hex e8');
  await expect(decOf(page, 0)).toHaveText(', decimal 232');
  await expect(hexOf(page, 3)).toHaveText('Byte 3 is hex 5a');

  await page.locator('#matrix .bit[data-k="0"]').click();
  await expect(hexOf(page, 0)).toHaveText('Byte 0 is hex e9');
  await expect(decOf(page, 0)).toHaveText(', decimal 233');

  await page.locator('#hex-input').fill('ff 10');
  await expect(hexOf(page, 0)).toHaveText('Byte 0 is hex ff');
  await expect(decOf(page, 1)).toHaveText(', decimal 16');
  await expect(decOf(page, 7)).toHaveText(', decimal 0');
});

test('runs of three or more bits of one signal carry its name', async ({ page }) => {
  // DriveMode (2 bits) and BrakePressed (1 bit) are too short to name.
  await expect(names(page)).toHaveText([
    'VehicleSpeed',
    'VehicleSpeed',
    'GearPosition',
    'AcceleratorPedal',
    'SteeringAngle',
    'SteeringAngle',
    'Checksum',
    'RollingCounter',
  ]);
});

test('names follow a Motorola signal across bytes and cover exactly its bits', async ({ page }) => {
  await open(page, 'WheelSpeeds');
  await expect(names(page)).toHaveText([
    'WheelSpeedFL',
    'WheelSpeedFL',
    'WheelSpeedFR',
    'WheelSpeedFR',
    'WheelSpeedRL',
    'WheelSpeedRL',
    'WheelSpeedRR',
    'WheelSpeedRR',
    'YawRate',
    'YawRate',
  ]);
  // In byte 1, WheelSpeedFL ends at bit 4 and WheelSpeedFR fills bits 3 to 0.
  const box = (k) => page.locator(`#matrix .bit[data-k="${k}"]`).boundingBox();
  const fr = await names(page).nth(2).boundingBox();
  const [bit3, bit0] = [await box(8 + 3), await box(8 + 0)];
  expect(Math.abs(fr.x - bit3.x)).toBeLessThan(0.5);
  expect(Math.abs(fr.x + fr.width - (bit0.x + bit0.width))).toBeLessThan(0.5);
  expect(Math.abs(fr.y - bit3.y)).toBeLessThan(0.5);
});

test('a bit under a name still flips when clicked', async ({ page }) => {
  // Byte 0, bit 7 sits under the VehicleSpeed label. Playwright refuses to
  // click an element that something else would catch the click for.
  await page.locator('#matrix .bit[data-k="7"]').click();
  await expect(page.locator('#matrix .bit[data-k="7"]')).toHaveAttribute('aria-label', /^Byte 0, bit 7 is 0,/);
  await expect(signalRow(page, 'VehicleSpeed').locator('td').nth(1)).toHaveText('8.72 km/h');
});

test('names follow the multiplexer page', async ({ page }) => {
  await open(page, 'InverterTelemetry');
  await expect(names(page)).toHaveText(['PageIndex', 'MotorSpeed', 'MotorSpeed', 'MotorTorque', 'MotorTorque', 'InverterState']);
  await page.getByRole('button', { name: '1: Temperatures' }).click();
  await expect(names(page)).toHaveText(['PageIndex', 'StatorTemp', 'IgbtTemp', 'InverterState']);
});

test('a grid updated in place after flips matches a freshly drawn one', async ({ page }) => {
  // A flip patches only the bits that changed; drawing the grid again from
  // the same bytes must give exactly the same cells.
  await open(page, 'DiagnosticFD');
  await page.getByRole('button', { name: 'Random' }).click();
  for (const k of [0, 9, 63, 100, 511]) await page.locator(`#matrix .bit[data-k="${k}"]`).click();
  const cells = () =>
    page.$$eval('#matrix .bit, #matrix .byte-value:not(.colhead)', (els) =>
      els.map((e) => {
        const classes = [...e.classList].filter((c) => c !== 'dim').sort().join(' ');
        return [classes, e.textContent, e.getAttribute('aria-label')].join('|');
      }),
    );
  const patched = await cells();
  await open(page, 'VehicleStatus');
  await open(page, 'DiagnosticFD');
  expect(await cells()).toEqual(patched);
  expect(patched).toHaveLength(64 * 8 + 64 * 2);
});

test('the highlight of the focused signal survives a flip', async ({ page }) => {
  // Byte 4, bit 0 belongs to SteeringAngle. Enter flips the focused bit.
  await page.locator('#matrix .bit[data-k="32"]').focus();
  await page.keyboard.press('Enter');
  await expect(page.locator('#matrix .bit[data-k="32"]')).toHaveAttribute('aria-label', /^Byte 4, bit 0 is 1,/);
  await expect(page.locator('#matrix .bit[data-k="32"]')).toBeFocused();
  await expect(signalRow(page, 'SteeringAngle')).toHaveClass(/\bhot\b/);
  await expect(page.locator('#matrix .bit[data-k="0"]')).toHaveClass(/\bdim\b/);
});

test('pointing at a signal dims the other bits and names, as soon as the grid appears', async ({ page }) => {
  await signalRow(page, 'SteeringAngle').hover();
  const opacity = (selector) => page.locator(selector).first().evaluate((el) => getComputedStyle(el).opacity);
  await expect.poll(() => opacity('#matrix .bit[data-k="0"]')).toBe('0.25');
  await expect.poll(() => opacity('#matrix .run:has-text("VehicleSpeed")')).toBe('0.25');
  await expect.poll(() => opacity('#matrix .bit[data-k="32"]')).toBe('1');
  await expect.poll(() => opacity('#matrix .run:has-text("SteeringAngle")')).toBe('1');
});
