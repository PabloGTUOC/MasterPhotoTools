/**
 * RP-4 Colour Tools acceptance & verification script in a real headless browser.
 *
 * Verifies:
 * 1. api.ts exports whiteBalanceAt, autoTone, BlackAndWhite, WhiteBalancePick, AutoTone.
 * 2. Edit.vue references whiteBalanceAt and autoTone.
 * 3. Edit.vue binds W shortcut for eyedropper and V shortcut for B&W toggle.
 * 4. Edit.vue contains eyedropper button, crosshair cursor binding, refusal handling.
 * 5. Edit.vue contains Auto button with undo notice.
 * 6. Edit.vue contains B&W toggle and 8 mix sliders.
 * 7. All 8 slider labels present: Red, Orange, Yellow, Green, Aqua, Blue, Purple, Magenta.
 * 8. HSL and B&W mix swap cleanly based on B&W state.
 * 9. Eyedropper sampling updates WB sliders and provides Undo.
 * 10. Auto button updates exposure, whites, and blacks, and provides Undo.
 * 11. B&W toggle disables saturation and displays all 8 mixer sliders.
 */

import { existsSync, mkdirSync, readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath, URL } from 'node:url';
import { createServer } from 'vite';
import vue from '@vitejs/plugin-vue';
import { chromium } from 'playwright';

function preinstalledChromium() {
  const root = process.env.PLAYWRIGHT_BROWSERS_PATH ?? '/opt/pw-browsers';
  if (!existsSync(root)) return undefined;
  for (const dir of readdirSync(root)) {
    for (const rel of ['chrome-linux/chrome', 'chrome-linux/headless_shell']) {
      const candidate = join(root, dir, rel);
      if (existsSync(candidate)) return candidate;
    }
  }
  return undefined;
}

const OUT = fileURLToPath(new URL('../layout-proof', import.meta.url));
mkdirSync(OUT, { recursive: true });

const ui = fileURLToPath(new URL('../../shared/src/ui', import.meta.url));
const harness = fileURLToPath(new URL('./edit-harness', import.meta.url));
const stubApi = fileURLToPath(new URL('./edit-harness/stub-api.ts', import.meta.url));
const hostSrc = fileURLToPath(new URL('../src', import.meta.url));

console.log('--- Phase 1: Static Contract Verification ---');

const apiPath = fileURLToPath(new URL('../src/api.ts', import.meta.url));
const editPath = fileURLToPath(new URL('../src/views/Edit.vue', import.meta.url));

const apiSource = readFileSync(apiPath, 'utf8');
const editSource = readFileSync(editPath, 'utf8');

const staticFailures = [];

function checkStatic(desc, condition) {
  if (condition) {
    console.log(`  ✓ ${desc}`);
  } else {
    console.error(`  ✗ ${desc}`);
    staticFailures.push(desc);
  }
}

// 1. api.ts exports
checkStatic("api.ts exports whiteBalanceAt method", /whiteBalanceAt\s*\(/.test(apiSource));
checkStatic("api.ts exports autoTone method", /autoTone\s*\(/.test(apiSource));
checkStatic("api.ts exports BlackAndWhite type", /export\s+interface\s+BlackAndWhite/.test(apiSource));
checkStatic("api.ts exports WhiteBalancePick type", /export\s+interface\s+WhiteBalancePick/.test(apiSource));
checkStatic("api.ts exports AutoTone type", /export\s+interface\s+AutoTone/.test(apiSource));
checkStatic("api.ts AdjustmentRecipe has black_and_white field", /black_and_white\?\s*:\s*BlackAndWhite/.test(apiSource));

// 2. Edit.vue references
checkStatic("Edit.vue references whiteBalanceAt", /whiteBalanceAt/.test(editSource));
checkStatic("Edit.vue references autoTone", /autoTone/.test(editSource));

// 3. Shortcuts W and V
checkStatic("Edit.vue binds W shortcut for eyedropper", /e\.key\s*===\s*['"]w['"]/i.test(editSource));
checkStatic("Edit.vue binds V shortcut for B&W toggle", /e\.key\s*===\s*['"]v['"]/i.test(editSource));

// 4. Eyedropper UI & refusal
checkStatic("Edit.vue contains eyedropper button", /data-testid="eyedropper-btn"/.test(editSource));
checkStatic("Edit.vue binds crosshair cursor", /is-eyedropper|cursor-crosshair/.test(editSource));
checkStatic("Edit.vue handles eyedropper refusal message", /Cannot balance here/.test(editSource));

// 5. Auto button with undo notice
checkStatic("Edit.vue contains Auto button", /data-testid="auto-tone-btn"/.test(editSource));
checkStatic("Edit.vue shows undo notice for Auto Tone", /Auto Tone:/.test(editSource));

// 6 & 7. B&W toggle and 8 mix sliders
checkStatic("Edit.vue contains B&W toggle", /data-testid="bw-toggle-btn"/.test(editSource));
checkStatic("Edit.vue contains B&W mix panel", /data-testid="bw-mix-panel"/.test(editSource));

const bwBands = ['Red', 'Orange', 'Yellow', 'Green', 'Aqua', 'Blue', 'Purple', 'Magenta'];
for (const band of bwBands) {
  checkStatic(`Edit.vue contains ${band} band in B&W mixer`, new RegExp(`label:\\s*['"]${band}['"]`).test(editSource));
}

// 8. HSL and B&W mix swap
checkStatic("Edit.vue swaps between B&W mix and HSL", /v-if="isBwActive"[\s\S]*v-else/.test(editSource));

if (staticFailures.length > 0) {
  console.error(`\nStatic checks failed: ${staticFailures.length} errors.`);
  process.exit(1);
}

console.log('\n--- Phase 2: In-Browser Acceptance Verification ---');

console.log('Starting Vite harness server...');
const server = await createServer({
  root: harness,
  configFile: false,
  plugins: [vue()],
  resolve: {
    dedupe: ['vue', 'vue-router'],
    alias: [
      { find: /^@ui\//, replacement: `${ui}/` },
      { find: /^@host\/api$/, replacement: stubApi },
      { find: /^@host\//, replacement: `${hostSrc}/` },
    ],
  },
  server: {
    fs: {
      allow: [
        harness,
        ui,
        fileURLToPath(new URL('..', import.meta.url)),
        fileURLToPath(new URL('../..', import.meta.url)),
      ],
    },
  },
  logLevel: 'warn',
});
await server.listen();

const bound = server.httpServer.address();
const host = bound.family === 'IPv6' ? `[${bound.address}]` : bound.address;
const base = `http://${host}:${bound.port}`;

const browser = await chromium.launch({
  executablePath: preinstalledChromium(),
  headless: true,
});
const page = await browser.newPage({
  viewport: { width: 1440, height: 900 },
  deviceScaleFactor: 2,
});
page.setDefaultTimeout(15000);

const testFailures = [];
function check(desc, condition) {
  if (condition) {
    console.log(`  ✓ ${desc}`);
  } else {
    console.error(`  ✗ ${desc}`);
    testFailures.push(desc);
  }
}

try {
  await page.goto(base, { waitUntil: 'networkidle' });

  // Open photograph
  const pathInput = page.locator('input[placeholder="/path/to/image.jpg"]');
  await pathInput.fill('/Volumes/Photos/sample.jpg');
  await pathInput.press('Enter');

  // Wait for canvas
  await page.waitForSelector('canvas[data-testid="edit-canvas"]');
  await page.waitForTimeout(300);
  await pathInput.blur();
  await page.keyboard.press('Escape'); // clear any lingering focus

  // 1. Eyedropper test via button and W shortcut
  console.log('Testing Eyedropper tool...');
  const eyedropperBtn = page.locator('button[data-testid="eyedropper-btn"]');
  await eyedropperBtn.waitFor({ state: 'visible' });

  // Toggle with W
  await page.keyboard.press('w');
  await page.waitForTimeout(100);
  let isActive = await eyedropperBtn.evaluate((el) => el.classList.contains('active'));
  check('Pressing W activates eyedropper button', isActive);

  const container = page.locator('.canvas-viewport__container');
  let hasCrosshair = await container.evaluate((el) => el.classList.contains('is-eyedropper'));
  check('Viewport has is-eyedropper crosshair class', hasCrosshair);

  // Press Escape to cancel
  await page.keyboard.press('Escape');
  await page.waitForTimeout(100);
  isActive = await eyedropperBtn.evaluate((el) => el.classList.contains('active'));
  check('Pressing Escape deactivates eyedropper', !isActive);

  // Click eyedropper button to activate
  await eyedropperBtn.click();
  await page.waitForTimeout(100);
  isActive = await eyedropperBtn.evaluate((el) => el.classList.contains('active'));
  check('Clicking button activates eyedropper', isActive);

  // Click on canvas to sample
  const canvas = page.locator('canvas[data-testid="edit-canvas"]');
  const box = await canvas.boundingBox();
  await page.mouse.click(box.x + box.width * 0.5, box.y + box.height * 0.5);
  await page.waitForTimeout(200);

  // Verify eyedropper deactivated after sample
  isActive = await eyedropperBtn.evaluate((el) => el.classList.contains('active'));
  check('Eyedropper deactivates after sampling point', !isActive);

  // Verify notice appeared with Undo button
  const notice = page.locator('[data-testid="edit-notice"]');
  await notice.waitFor({ state: 'visible' });
  const noticeText = await notice.innerText();
  check('Notice reports white balance shift', noticeText.includes('WB: Temp'));

  const undoBtn = page.locator('[data-testid="edit-notice-undo"]');
  check('Notice contains Undo button', await undoBtn.isVisible());

  // Click Undo
  await undoBtn.click();
  await page.waitForTimeout(200);
  const tempInput = page.locator('[data-testid="temperature-number"]');
  const tempVal = await tempInput.inputValue();
  check('Undo restores previous temperature (0)', Number(tempVal) === 0);

  // 2. Auto Tone test
  console.log('Testing Auto Tone...');
  const autoBtn = page.locator('button[data-testid="auto-tone-btn"]');
  await autoBtn.waitFor({ state: 'visible' });
  await autoBtn.click();
  await page.waitForTimeout(200);

  // Notice reports Auto Tone
  await notice.waitFor({ state: 'visible' });
  const autoNoticeText = await notice.innerText();
  check('Notice reports Auto Tone shifts', autoNoticeText.includes('Auto Tone:'));

  const expInput = page.locator('[data-testid="exposure-number"]');
  const expVal = await expInput.inputValue();
  check('Auto Tone applied exposure shift', Number(expVal) !== 0);

  // Undo Auto Tone
  const undoAutoBtn = page.locator('[data-testid="edit-notice-undo"]');
  await undoAutoBtn.click();
  await page.waitForTimeout(200);
  const expValUndone = await expInput.inputValue();
  check('Undo restores previous exposure (0.0)', Number(expValUndone) === 0);

  // 3. Black & White mode test
  console.log('Testing Black & White mode...');
  // Open Colour section
  const colourSectionToggle = page.locator('[data-testid="section-hsl-toggle"]');
  await colourSectionToggle.click();
  await page.waitForTimeout(200);

  // Verify HSL panel is visible initially
  const hslPanel = page.locator('[data-testid="hsl-panel"]');
  check('HSL panel is visible before B&W toggle', await hslPanel.isVisible());

  // Press V to toggle B&W
  await page.keyboard.press('v');
  await page.waitForTimeout(200);

  const bwToggleBtn = page.locator('button[data-testid="bw-toggle-btn"]');
  const isBwBtnActive = await bwToggleBtn.evaluate((el) => el.classList.contains('active'));
  check('Pressing V activates Black & White button', isBwBtnActive);

  // Check HSL is hidden and B&W mix is visible
  const bwMixPanel = page.locator('[data-testid="bw-mix-panel"]');
  check('B&W mix panel is visible', await bwMixPanel.isVisible());
  check('HSL panel is hidden when B&W is active', !(await hslPanel.isVisible()));

  // Check all 8 sliders in B&W mixer
  for (const band of ['red', 'orange', 'yellow', 'green', 'aqua', 'blue', 'purple', 'magenta']) {
    const slider = page.locator(`[data-testid="bw-slider-${band}-slider"]`);
    check(`B&W mixer slider for ${band} is present`, await slider.isVisible());
  }

  // Check Saturation slider is disabled in B&W mode
  const satInput = page.locator('[data-testid="saturation-slider"]');
  check('Saturation slider is disabled in B&W mode', await satInput.isDisabled());

  // Press V to toggle B&W back off
  await page.keyboard.press('v');
  await page.waitForTimeout(200);
  check('HSL panel is restored when B&W toggled off', await hslPanel.isVisible());
  check('B&W mix panel is hidden', !(await bwMixPanel.isVisible()));
  check('Saturation slider is re-enabled', !(await satInput.isDisabled()));

  // Take proof screenshot
  await page.screenshot({ path: join(OUT, 'colour-tools.png') });
  console.log('Proof screenshot saved to layout-proof/colour-tools.png');

} catch (err) {
  console.error('In-browser test error:', err);
  testFailures.push(String(err));
} finally {
  await browser.close();
  await server.close();
}

if (testFailures.length > 0) {
  console.error(`\ncheck:colour FAILED: ${testFailures.length} issues detected.`);
  process.exit(1);
}

console.log('\ncheck:colour PASSED: all 11 colour tool checks passed cleanly.');
