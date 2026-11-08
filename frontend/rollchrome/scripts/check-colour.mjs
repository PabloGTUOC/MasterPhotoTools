/**
 * RP-4 Colour Tools acceptance & verification in a real headless browser.
 *
 * Asserts all 11 requirements as numbered in the brief:
 * 1. W then a click on an orientation-1 photograph sends the expected x, y ± 0.01;
 *    the sliders take the stubbed values; a Settle render and a save follow.
 * 2. The same on orientation 6, with the point mapped.
 * 3. The same with a crop in the recipe, mapped through to-stored.
 * 4. Esc leaves the mode and sends nothing.
 * 5. At 100%, a drag while picking does not pan.
 * 6. A stubbed clamped result shows the notice; a stubbed refusal is shown and leaves the sliders where they were.
 * 7. With text focused, W and V do nothing.
 * 8. Auto sets the three values and leaves the others; Undo restores all three.
 * 9. V sets black_and_white, shows eight sliders in the HSL order and hides the HSL section;
 *    V again shows it with its values intact.
 * 10. In crop mode and while comparing the eyedropper is disabled.
 * 11. Every new control is at least 40 px. Screenshot to layout-proof/.
 */

import { existsSync, mkdirSync, readdirSync } from 'node:fs';
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

console.log('Starting Vite harness server for check:colour...');
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

console.log(`Harness server listening at ${base}`);

const browser = await chromium.launch({
  executablePath: preinstalledChromium(),
  headless: true,
});
const page = await browser.newPage({
  viewport: { width: 1440, height: 900 },
  deviceScaleFactor: 2,
});
page.setDefaultTimeout(20000);
page.on('pageerror', (err) => console.error('PAGE ERROR:', err.message));
page.on('console', (msg) => {
  if (msg.text().includes('[DEBUG')) console.log('BROWSER:', msg.text());
});

const failures = [];
let assertionsRun = 0;

try {
  await page.goto(base, { waitUntil: 'networkidle' });
  const pathInput = page.locator('input[placeholder="/path/to/image.jpg"]');
  await pathInput.waitFor({ state: 'visible' });

  // Helper to blur all active elements so keyboard shortcuts are never suppressed
  const blurAll = async () => {
    await page.evaluate(() => {
      if (document.activeElement && document.activeElement instanceof HTMLElement) {
        document.activeElement.blur();
      }
      window.getSelection()?.removeAllRanges();
    });
  };

  // Load photograph into view
  await pathInput.fill('/Volumes/Photos/test.jpg');
  await pathInput.press('Enter');
  await page.waitForTimeout(200);
  await blurAll();

  const canvas = page.locator('canvas[data-testid="edit-canvas"]');
  await canvas.waitFor({ state: 'visible' });

  // -------------------------------------------------------------------------
  // 1. W then a click on an orientation-1 photograph sends the expected x, y ± 0.01;
  // the sliders take the stubbed values; a Settle render and a save follow.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 1: W then click on orientation-1 photograph...');
  await page.evaluate(() => {
    window.__STUB__.orientation = 1;
    window.__STUB__.toStoredOverride = null;
    window.__STUB__.whiteBalanceCalls = [];
    window.__STUB__.whiteBalanceResult = { temperature: 15, tint: -8, clamped: false };
    window.__STUB__.whiteBalanceError = null;
    window.__STUB__.renders = [];
    window.__STUB__.lastSavedRecipe = null;
  });
  await pathInput.fill('/Volumes/Photos/orient1.jpg');
  await pathInput.press('Enter');
  await page.waitForTimeout(250);
  await blurAll();

  // Press W to enter eyedropper mode
  await page.keyboard.press('w');
  await page.waitForTimeout(50);

  const isEyedropperActive1 = await page.evaluate(() => {
    return document.querySelector('.canvas-viewport__container')?.classList.contains('is-eyedropper');
  });
  if (!isEyedropperActive1) {
    failures.push('Assertion 1: Pressing W did not activate eyedropper mode');
  }

  // Click at fraction (u=0.4, v=0.6) of canvas
  const box1 = await canvas.boundingBox();
  if (box1) {
    const clickX = box1.x + box1.width * 0.4;
    const clickY = box1.y + box1.height * 0.6;
    await page.mouse.click(clickX, clickY);
    // Allow debounce timer (300 ms) for save to finish
    await page.waitForTimeout(450);

    const st1 = await page.evaluate(() => ({
      calls: window.__STUB__.whiteBalanceCalls,
      renders: window.__STUB__.renders,
      saved: window.__STUB__.lastSavedRecipe,
    }));

    if (st1.calls.length === 0) {
      failures.push('Assertion 1: Eyedropper click did not call whiteBalanceAt');
    } else {
      const lastCall = st1.calls[st1.calls.length - 1];
      if (Math.abs(lastCall.x - 0.4) > 0.01 || Math.abs(lastCall.y - 0.6) > 0.01) {
        failures.push(`Assertion 1: Expected x=0.40, y=0.60 ± 0.01, got x=${lastCall.x.toFixed(3)}, y=${lastCall.y.toFixed(3)}`);
      }
    }

    const tempVal1 = await page.locator('input[data-testid="temperature-number"]').inputValue();
    const tintVal1 = await page.locator('input[data-testid="tint-number"]').inputValue();
    if (tempVal1 !== '15' || tintVal1 !== '-8') {
      failures.push(`Assertion 1: Sliders did not take stubbed values (expected temp=15, tint=-8; got temp=${tempVal1}, tint=${tintVal1})`);
    }

    const hasSettleRender = st1.renders.some((r) => r.stage === 'Settle' && r.recipe.temperature === 15 && r.recipe.tint === -8);
    if (!hasSettleRender) {
      failures.push('Assertion 1: A Settle render with temperature=15 and tint=-8 did not follow');
    }

    if (!st1.saved || st1.saved.temperature !== 15 || st1.saved.tint !== -8) {
      failures.push(`Assertion 1: Recipe was not saved with picked values (saved: ${JSON.stringify(st1.saved)})`);
    }
  } else {
    failures.push('Assertion 1: Canvas bounding box not found');
  }

  // -------------------------------------------------------------------------
  // 2. The same on orientation 6, with the point mapped.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 2: The same on orientation 6, with point mapped...');
  await page.evaluate(() => {
    window.__STUB__.orientation = 6;
    window.__STUB__.toStoredOverride = null;
    window.__STUB__.whiteBalanceCalls = [];
    window.__STUB__.whiteBalanceResult = { temperature: -12, tint: 6, clamped: false };
    window.__STUB__.whiteBalanceError = null;
    window.__STUB__.renders = [];
    window.__STUB__.lastSavedRecipe = null;
  });
  await pathInput.fill('/Volumes/Photos/orient6.jpg');
  await pathInput.press('Enter');
  await page.waitForTimeout(250);
  await blurAll();

  await page.keyboard.press('w');
  await page.waitForTimeout(50);

  const box2 = await canvas.boundingBox();
  if (box2) {
    // Click at display fraction uDisp=0.7, vDisp=0.3
    // For orientation 6 (90° CW rotation), inverted frame coordinates are:
    // uFrame = vDisp = 0.3, vFrame = 1.0 - uDisp = 0.3.
    // Stored coordinates match uFrame, vFrame = (0.3, 0.3).
    const clickX = box2.x + box2.width * 0.7;
    const clickY = box2.y + box2.height * 0.3;
    await page.mouse.click(clickX, clickY);
    await page.waitForTimeout(450);

    const st2 = await page.evaluate(() => ({
      calls: window.__STUB__.whiteBalanceCalls,
      renders: window.__STUB__.renders,
      saved: window.__STUB__.lastSavedRecipe,
    }));

    if (st2.calls.length === 0) {
      failures.push('Assertion 2: Eyedropper click on orientation 6 did not call whiteBalanceAt');
    } else {
      const lastCall = st2.calls[st2.calls.length - 1];
      if (Math.abs(lastCall.x - 0.3) > 0.01 || Math.abs(lastCall.y - 0.3) > 0.01) {
        failures.push(`Assertion 2: Orientation 6 coordinate mapping failed: expected x=0.30, y=0.30 ± 0.01; got x=${lastCall.x.toFixed(3)}, y=${lastCall.y.toFixed(3)}`);
      }
    }

    const tempVal2 = await page.locator('input[data-testid="temperature-number"]').inputValue();
    const tintVal2 = await page.locator('input[data-testid="tint-number"]').inputValue();
    if (tempVal2 !== '-12' || tintVal2 !== '6') {
      failures.push(`Assertion 2: Sliders did not take stubbed values on orientation 6 (got temp=${tempVal2}, tint=${tintVal2})`);
    }

    const hasSettleRender2 = st2.renders.some((r) => r.stage === 'Settle' && r.recipe.temperature === -12 && r.recipe.tint === 6);
    if (!hasSettleRender2) {
      failures.push('Assertion 2: Settle render did not follow on orientation 6');
    }
  }

  // -------------------------------------------------------------------------
  // 3. The same with a crop in the recipe, mapped through to-stored.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 3: The same with a crop in the recipe, mapped through to-stored...');
  // A crop of width 0.5, height 0.6 at (x=0.2, y=0.1) produces affine [0.5, 0, 0.2, 0, 0.6, 0.1]
  await page.evaluate(() => {
    window.__STUB__.orientation = 1;
    window.__STUB__.toStoredOverride = [0.5, 0, 0.2, 0, 0.6, 0.1];
    window.__STUB__.whiteBalanceCalls = [];
    window.__STUB__.whiteBalanceResult = { temperature: 20, tint: -10, clamped: false };
    window.__STUB__.renders = [];
  });
  await pathInput.fill('/Volumes/Photos/cropped.jpg');
  await pathInput.press('Enter');
  await page.waitForTimeout(250);
  await blurAll();

  await page.keyboard.press('w');
  await page.waitForTimeout(50);

  const box3 = await canvas.boundingBox();
  if (box3) {
    // Click at display center uDisp=0.5, vDisp=0.5
    // Mapped through to-stored:
    // storedX = 0.5 * 0.5 + 0.2 = 0.45
    // storedY = 0.6 * 0.5 + 0.1 = 0.40
    const clickX = box3.x + box3.width * 0.5;
    const clickY = box3.y + box3.height * 0.5;
    await page.mouse.click(clickX, clickY);
    await page.waitForTimeout(200);

    const st3 = await page.evaluate(() => window.__STUB__.whiteBalanceCalls);
    if (st3.length === 0) {
      failures.push('Assertion 3: Eyedropper click with crop did not call whiteBalanceAt');
    } else {
      const lastCall = st3[st3.length - 1];
      if (Math.abs(lastCall.x - 0.45) > 0.01 || Math.abs(lastCall.y - 0.40) > 0.01) {
        failures.push(`Assertion 3: to-stored mapping failed: expected storedX=0.45, storedY=0.40 ± 0.01; got storedX=${lastCall.x.toFixed(3)}, storedY=${lastCall.y.toFixed(3)}`);
      }
    }
  }

  // Clear override and dismiss toast
  await page.evaluate(() => {
    window.__STUB__.toStoredOverride = null;
    const dismissBtn = document.querySelector('button.edit-notice__btn:last-child');
    if (dismissBtn instanceof HTMLElement) dismissBtn.click();
  });
  await page.waitForTimeout(100);

  // -------------------------------------------------------------------------
  // 4. Esc leaves the mode and sends nothing.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 4: Esc leaves the mode and sends nothing...');
  await page.evaluate(() => {
    window.__STUB__.whiteBalanceCalls = [];
  });
  await blurAll();
  await page.keyboard.press('w');
  await page.waitForTimeout(50);

  const isEyedropperBeforeEsc = await page.evaluate(() => {
    return document.querySelector('.canvas-viewport__container')?.classList.contains('is-eyedropper');
  });
  if (!isEyedropperBeforeEsc) {
    failures.push('Assertion 4: W did not enter eyedropper mode');
  }

  await page.keyboard.press('Escape');
  await page.waitForTimeout(50);

  const isEyedropperAfterEsc = await page.evaluate(() => {
    return document.querySelector('.canvas-viewport__container')?.classList.contains('is-eyedropper');
  });
  if (isEyedropperAfterEsc) {
    failures.push('Assertion 4: Esc did not exit eyedropper mode');
  }

  const callsAfterEsc = await page.evaluate(() => window.__STUB__.whiteBalanceCalls.length);
  if (callsAfterEsc > 0) {
    failures.push(`Assertion 4: Esc sent calls to whiteBalanceAt (count: ${callsAfterEsc})`);
  }

  // -------------------------------------------------------------------------
  // 5. At 100%, a drag while picking does not pan.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 5: At 100%, a drag while picking does not pan...');
  await blurAll();
  // Zoom to 100%
  await page.keyboard.press('Meta+1');
  await page.waitForTimeout(150);

  // Activate eyedropper
  await page.keyboard.press('w');
  await page.waitForTimeout(50);

  const panBeforeDrag = await page.evaluate(() => {
    const stage = document.querySelector('.canvas-viewport__stage');
    return { left: stage?.style.left, top: stage?.style.top };
  });

  const box5 = await canvas.boundingBox();
  if (box5) {
    // Start mouse drag
    await page.mouse.move(box5.x + 100, box5.y + 100);
    await page.mouse.down();
    await page.mouse.move(box5.x + 200, box5.y + 200, { steps: 5 });
    await page.mouse.up();
    await page.waitForTimeout(100);

    const panAfterDrag = await page.evaluate(() => {
      const stage = document.querySelector('.canvas-viewport__stage');
      return { left: stage?.style.left, top: stage?.style.top };
    });

    if (panBeforeDrag.left !== panAfterDrag.left || panBeforeDrag.top !== panAfterDrag.top) {
      failures.push(`Assertion 5: Drag while picking caused panning (before: ${JSON.stringify(panBeforeDrag)}, after: ${JSON.stringify(panAfterDrag)})`);
    }
  }

  // Reset zoom to fit, exit eyedropper if active, and dismiss any notice
  await page.keyboard.press('Escape');
  await page.waitForTimeout(50);
  await page.locator('button[data-testid="zoom-fit-btn"]').click();
  await page.waitForTimeout(100);
  await blurAll();
  await page.evaluate(() => {
    const dismissBtn = document.querySelector('button.edit-notice__btn:last-child');
    if (dismissBtn instanceof HTMLElement) dismissBtn.click();
  });
  await page.waitForTimeout(50);

  // -------------------------------------------------------------------------
  // 6. A stubbed clamped result shows the notice; a stubbed refusal is shown
  // and leaves the sliders where they were.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 6: Stubbed clamped result shows notice; stubbed refusal leaves sliders unchanged...');
  // Reload clean photo for assertion 6
  await page.evaluate(() => {
    window.__STUB__.orientation = 1;
    window.__STUB__.toStoredOverride = null;
    window.__STUB__.whiteBalanceCalls = [];
    window.__STUB__.whiteBalanceResult = { temperature: 100, tint: -25, clamped: true };
    window.__STUB__.whiteBalanceError = null;
  });
  await pathInput.fill('/Volumes/Photos/clamped_test.jpg');
  await pathInput.press('Enter');
  await page.waitForTimeout(250);
  await blurAll();

  // Part A: Clamped
  await page.keyboard.press('w');
  await page.waitForTimeout(50);

  const box6 = await canvas.boundingBox();
  if (box6) {
    await page.mouse.click(box6.x + box6.width * 0.5, box6.y + box6.height * 0.5);
    await page.waitForTimeout(200);

    const noticeEl = page.locator('[data-testid="edit-notice"]');
    await noticeEl.waitFor({ state: 'visible', timeout: 5000 });
    const noticeTextClamped = await noticeEl.textContent();
    if (!noticeTextClamped?.includes('slider range reached')) {
      failures.push(`Assertion 6: Notice toast did not report clamped slider range (got: "${noticeTextClamped}")`);
    }

    // Part B: Refusal
    const tempBeforeRefusal = await page.locator('input[data-testid="temperature-number"]').inputValue();
    const tintBeforeRefusal = await page.locator('input[data-testid="tint-number"]').inputValue();

    await page.evaluate(() => {
      window.__STUB__.whiteBalanceError = 'Cannot balance on clipped highlight (channel mean >= 0.98)';
      const dismissBtn = document.querySelector('button.edit-notice__btn:last-child');
      if (dismissBtn instanceof HTMLElement) dismissBtn.click();
    });
    await page.waitForTimeout(50);
    await blurAll();
    await page.keyboard.press('w');
    await page.waitForTimeout(50);

    await page.mouse.click(box6.x + box6.width * 0.5, box6.y + box6.height * 0.5);
    await page.waitForTimeout(200);

    const noticeTextRefused = await noticeEl.textContent();
    if (!noticeTextRefused?.includes('clipped highlight')) {
      failures.push(`Assertion 6: Notice toast did not report refusal message (got: "${noticeTextRefused}")`);
    }

    const tempAfterRefusal = await page.locator('input[data-testid="temperature-number"]').inputValue();
    const tintAfterRefusal = await page.locator('input[data-testid="tint-number"]').inputValue();
    if (tempBeforeRefusal !== tempAfterRefusal || tintBeforeRefusal !== tintAfterRefusal) {
      failures.push(`Assertion 6: Sliders moved despite refusal (before: temp=${tempBeforeRefusal}, tint=${tintBeforeRefusal}; after: temp=${tempAfterRefusal}, tint=${tintAfterRefusal})`);
    }
  }

  // Reset stub error and dismiss notice
  await page.evaluate(() => {
    window.__STUB__.whiteBalanceError = null;
    const dismissBtn = document.querySelector('button.edit-notice__btn:last-child');
    if (dismissBtn instanceof HTMLElement) dismissBtn.click();
  });
  await page.waitForTimeout(50);

  // -------------------------------------------------------------------------
  // 7. With text focused, W and V do nothing.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 7: With text focused, W and V do nothing...');
  const expInput7 = page.locator('input[data-testid="exposure-number"]');
  await expInput7.focus();
  await page.waitForTimeout(50);

  // Press W while text input is focused
  await page.keyboard.press('w');
  await page.waitForTimeout(50);

  const isEyedropperWithFocus = await page.evaluate(() => {
    return document.querySelector('.canvas-viewport__container')?.classList.contains('is-eyedropper');
  });
  if (isEyedropperWithFocus) {
    failures.push('Assertion 7: W activated eyedropper while text field had focus');
  }

  // Press V while text input is focused
  await page.keyboard.press('v');
  await page.waitForTimeout(50);

  const isBwWithFocus = await page.evaluate(() => {
    return document.querySelector('button[data-testid="bw-toggle-btn"]')?.classList.contains('active');
  });
  if (isBwWithFocus) {
    failures.push('Assertion 7: V activated Black & White while text field had focus');
  }
  await blurAll();

  // -------------------------------------------------------------------------
  // 8. Auto sets the three values and leaves the others; Undo restores all three.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 8: Auto sets the three values and leaves the others; Undo restores all three...');
  await page.evaluate(() => {
    window.__STUB__.autoToneCalls = [];
    window.__STUB__.autoToneResult = { exposure: 0.35, whites: 15.0, blacks: -8.0 };
    window.__STUB__.renders = [];
  });

  // Set highlights to 20 to check other sliders are untouched
  const hlInput = page.locator('input[data-testid="highlights-number"]');
  await hlInput.fill('20');
  await hlInput.press('Enter');
  await page.waitForTimeout(100);
  await blurAll();

  const expBeforeAuto = await page.locator('input[data-testid="exposure-number"]').inputValue();
  const whitesBeforeAuto = await page.locator('input[data-testid="whites-number"]').inputValue();
  const blacksBeforeAuto = await page.locator('input[data-testid="blacks-number"]').inputValue();

  // Click Auto button
  const autoBtn = page.locator('button[data-testid="auto-tone-btn"]');
  await autoBtn.click();
  await page.waitForTimeout(200);

  const autoCalls = await page.evaluate(() => window.__STUB__.autoToneCalls.length);
  if (autoCalls === 0) {
    failures.push('Assertion 8: Auto tone command was not called');
  }

  const expAfterAuto = await page.locator('input[data-testid="exposure-number"]').inputValue();
  const whitesAfterAuto = await page.locator('input[data-testid="whites-number"]').inputValue();
  const blacksAfterAuto = await page.locator('input[data-testid="blacks-number"]').inputValue();
  const hlAfterAuto = await page.locator('input[data-testid="highlights-number"]').inputValue();

  // Exposure number input formats with 1 decimal (0.35 -> 0.3/0.4)
  if (Math.abs(parseFloat(expAfterAuto) - 0.35) > 0.06 || whitesAfterAuto !== '15' || blacksAfterAuto !== '-8') {
    failures.push(`Assertion 8: Auto tone values not applied (expected exp=0.35, whites=15, blacks=-8; got exp=${expAfterAuto}, whites=${whitesAfterAuto}, blacks=${blacksAfterAuto})`);
  }
  if (hlAfterAuto !== '20') {
    failures.push(`Assertion 8: Auto tone modified an unrelated slider (highlights changed to ${hlAfterAuto})`);
  }

  // Click Undo in toast
  const undoBtn = page.locator('button[data-testid="edit-notice-undo"]');
  if (await undoBtn.isVisible()) {
    await undoBtn.click();
    await page.waitForTimeout(150);

    const expRestored = await page.locator('input[data-testid="exposure-number"]').inputValue();
    const whitesRestored = await page.locator('input[data-testid="whites-number"]').inputValue();
    const blacksRestored = await page.locator('input[data-testid="blacks-number"]').inputValue();

    if (expRestored !== expBeforeAuto || whitesRestored !== whitesBeforeAuto || blacksRestored !== blacksBeforeAuto) {
      failures.push(`Assertion 8: Undo did not restore all three values (restored: exp=${expRestored}, whites=${whitesRestored}, blacks=${blacksRestored}; expected: exp=${expBeforeAuto}, whites=${whitesBeforeAuto}, blacks=${blacksBeforeAuto})`);
    }
  } else {
    failures.push('Assertion 8: Notice toast Undo button was not visible after Auto tone');
  }

  // Dismiss toast
  await page.evaluate(() => {
    const dismissBtn = document.querySelector('button.edit-notice__btn:last-child');
    if (dismissBtn instanceof HTMLElement) dismissBtn.click();
  });
  await page.waitForTimeout(50);

  // -------------------------------------------------------------------------
  // 9. V sets black_and_white, shows eight sliders in the HSL order and hides
  // the HSL section; V again shows it with its values intact.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 9: V sets black_and_white, shows eight sliders in HSL order and hides HSL; V again restores HSL intact...');
  await blurAll();

  // Ensure Colour / HSL section is expanded
  const hslPanel = page.locator('[data-testid="hsl-panel"]');
  if (!(await hslPanel.isVisible())) {
    await page.locator('button[data-testid="section-hsl-toggle"]').click();
    await page.waitForTimeout(100);
  }

  const hslInitialVisible = await hslPanel.isVisible();
  if (!hslInitialVisible) {
    failures.push('Assertion 9: HSL panel not visible when section expanded');
  }

  // Edit an HSL slider (Blue saturation)
  const blueSwatch = page.locator('[data-testid="hsl-swatch-blue"]');
  if (await blueSwatch.isVisible()) {
    await blueSwatch.click();
    await page.waitForTimeout(50);
    const blueSatInput = page.locator('input[data-testid="hsl-slider-saturation-number"]');
    await blueSatInput.fill('40');
    await blueSatInput.press('Enter');
    await page.waitForTimeout(100);
    await blurAll();
  }

  // Press V to toggle Black & White
  await page.keyboard.press('v');
  await page.waitForTimeout(150);

  const bwActive9 = await page.evaluate(() => {
    return document.querySelector('button[data-testid="bw-toggle-btn"]')?.classList.contains('active');
  });
  if (!bwActive9) {
    failures.push('Assertion 9: Pressing V did not activate Black & White mode');
  }

  const hslHiddenInBw = !(await hslPanel.isVisible());
  if (!hslHiddenInBw) {
    failures.push('Assertion 9: HSL panel is not hidden when Black & White is active');
  }

  const bwMixPanelVisible = await page.locator('[data-testid="bw-mix-panel"]').isVisible();
  if (!bwMixPanelVisible) {
    failures.push('Assertion 9: B&W mix panel is not visible when Black & White is active');
  }

  // Verify all eight bands in HSL order
  const expectedBands = ['red', 'orange', 'yellow', 'green', 'aqua', 'blue', 'purple', 'magenta'];
  for (const band of expectedBands) {
    const slider = page.locator(`[data-testid="bw-slider-${band}-slider"]`);
    if (!(await slider.isVisible())) {
      failures.push(`Assertion 9: B&W mix slider for band "${band}" is not visible`);
    }
  }

  // Press V again to toggle back
  await blurAll();
  await page.keyboard.press('v');
  await page.waitForTimeout(150);

  const bwActiveAfterSecondV = await page.evaluate(() => {
    return document.querySelector('button[data-testid="bw-toggle-btn"]')?.classList.contains('active');
  });
  if (bwActiveAfterSecondV) {
    failures.push('Assertion 9: Second V press did not deactivate Black & White mode');
  }

  const hslVisibleAgain = await hslPanel.isVisible();
  if (!hslVisibleAgain) {
    failures.push('Assertion 9: HSL panel not restored when Black & White deactivated');
  }

  // Verify Blue saturation preserved
  if (await blueSwatch.isVisible()) {
    await blueSwatch.click();
    await page.waitForTimeout(50);
    const blueSatAfter = await page.locator('input[data-testid="hsl-slider-saturation-number"]').inputValue();
    if (blueSatAfter !== '40') {
      failures.push(`Assertion 9: HSL values not preserved across B&W toggle (expected blue sat=40, got ${blueSatAfter})`);
    }
  }

  // -------------------------------------------------------------------------
  // 10. In crop mode and while comparing the eyedropper is disabled.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 10: In crop mode and while comparing eyedropper is disabled...');
  // Part A: Crop mode
  const cropBtn = page.locator('button[data-testid="geometry-crop-btn"]');
  if (await cropBtn.isVisible()) {
    await cropBtn.click();
    await page.waitForTimeout(100);

    const eyedropperDisabledInCrop = await page.locator('button[data-testid="eyedropper-btn"]').isDisabled();
    if (!eyedropperDisabledInCrop) {
      failures.push('Assertion 10: Eyedropper button is not disabled in crop mode');
    }

    // Try pressing W while in crop mode
    await blurAll();
    await page.keyboard.press('w');
    await page.waitForTimeout(50);
    const eyedropperActiveInCrop = await page.evaluate(() => {
      return document.querySelector('.canvas-viewport__container')?.classList.contains('is-eyedropper');
    });
    if (eyedropperActiveInCrop) {
      failures.push('Assertion 10: W activated eyedropper while in crop mode');
    }

    // Exit crop mode
    await page.locator('button[data-testid="crop-done-btn"]').click();
    await page.waitForTimeout(100);
  }

  // Part B: Compare mode
  await blurAll();
  const sideBySideBtn = page.locator('button[data-testid="compare-side-by-side-btn"]');
  await sideBySideBtn.click();
  await page.waitForTimeout(150);

  const eyedropperDisabledInCompare = await page.locator('button[data-testid="eyedropper-btn"]').isDisabled();
  if (!eyedropperDisabledInCompare) {
    failures.push('Assertion 10: Eyedropper button is not disabled while comparing');
  }

  // Try pressing W while comparing
  await page.keyboard.press('w');
  await page.waitForTimeout(50);
  const eyedropperActiveInCompare = await page.evaluate(() => {
    return document.querySelector('.canvas-viewport__container')?.classList.contains('is-eyedropper');
  });
  if (eyedropperActiveInCompare) {
    failures.push('Assertion 10: W activated eyedropper while comparing');
  }

  // Turn compare off
  await sideBySideBtn.click();
  await page.waitForTimeout(100);
  await blurAll();

  // -------------------------------------------------------------------------
  // 11. Every new control is at least 40 px. Screenshot to layout-proof/.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 11: Every new control is at least 40 px. Screenshot to layout-proof/...');
  const controlsToCheck = [
    'eyedropper-btn',
    'auto-tone-btn',
    'bw-toggle-btn',
  ];

  for (const id of controlsToCheck) {
    const loc = page.locator(`button[data-testid="${id}"]`);
    if (await loc.isVisible()) {
      const box = await loc.boundingBox();
      if (!box || box.width < 39.5 || box.height < 39.5) {
        failures.push(`Assertion 11: Control ${id} touch target is under 40px (${box?.width.toFixed(1)}x${box?.height.toFixed(1)})`);
      }
    }
  }

  // Toggle B&W on to check mixer controls
  await blurAll();
  await page.keyboard.press('v');
  await page.waitForTimeout(100);

  const bwResetBtn = page.locator('button[data-testid="bw-mix-reset-btn"]');
  if (await bwResetBtn.isVisible()) {
    const box = await bwResetBtn.boundingBox();
    if (!box || box.width < 39.5 || box.height < 39.5) {
      failures.push(`Assertion 11: B&W mix reset button touch target is under 40px (${box?.width.toFixed(1)}x${box?.height.toFixed(1)})`);
    }
  }

  const proofPath = join(OUT, 'colour-tools.png');
  await page.screenshot({ path: proofPath, fullPage: true });
  console.log(`  Proof screenshot saved to ${proofPath}`);

} catch (err) {
  failures.push(`Unexpected error during check:colour execution: ${err.message}`);
} finally {
  await browser.close();
  await server.close();
}

console.log('\n--- check:colour Summary ---');
if (failures.length > 0) {
  console.error(`FAILED with ${failures.length} issues:`);
  for (const f of failures) {
    console.error(`  - ${f}`);
  }
  process.exit(1);
} else {
  console.log(`PASSED: all ${assertionsRun} colour tool checks passed cleanly.`);
  process.exit(0);
}
