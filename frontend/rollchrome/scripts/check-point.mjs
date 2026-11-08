/**
 * RP-10 Point Colour acceptance & verification in a real headless browser.
 *
 * Asserts all 11 requirements as numbered in the brief:
 * 1. Pick then a click sends colour_at with the mapped point, on orientation 1.
 * 2. The same on orientation 6.
 * 3. The new point appears, selected, with its four sliders at their defaults, and is in the save payload.
 * 4. Moving Saturation sends Drag renders carrying it; release sends Settle.
 * 5. The history label is Point colour; Cmd-Z takes it back.
 * 6. A stubbed refusal is shown and no point is added.
 * 7. Delete removes it from the save payload.
 * 8. At eight, Pick is disabled with the reason visible.
 * 9. The group is hidden while black & white is on, and the points are kept.
 * 10. W still sets white balance and adds no point.
 * 11. Every new control is at least 40 px. Screenshot to layout-proof/point-colour.png.
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

console.log('Starting Vite harness server for check:point...');
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

  const blurAll = async () => {
    await page.evaluate(() => {
      if (document.activeElement && document.activeElement instanceof HTMLElement) {
        document.activeElement.blur();
      }
      window.getSelection()?.removeAllRanges();
    });
  };

  const ensureHslSectionOpen = async () => {
    const section = page.locator('[data-testid="section-hsl"]');
    await section.waitFor({ state: 'visible' });
    const isOpen = await section.evaluate((el) => el.classList.contains('is-open'));
    if (!isOpen) {
      await page.locator('button[data-testid="section-hsl-toggle"]').click();
      await page.waitForTimeout(100);
    }
  };

  // -------------------------------------------------------------------------
  // 1. Pick then a click sends colour_at with the mapped point, on orientation 1.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 1: Pick then click sends colour_at with mapped point, on orientation 1...');
  await page.evaluate(() => {
    window.__STUB__.orientation = 1;
    window.__STUB__.toStoredOverride = null;
    window.__STUB__.colourAtCalls = [];
    window.__STUB__.colourAtResult = { hue: 45.0, chroma: 0.14, lightness: 0.62 };
    window.__STUB__.colourAtError = null;
    window.__STUB__.renders = [];
    window.__STUB__.lastSavedRecipe = null;
  });
  await pathInput.fill('/Volumes/Photos/orient1.jpg');
  await pathInput.press('Enter');
  await page.waitForTimeout(250);
  await blurAll();

  await ensureHslSectionOpen();

  const canvas = page.locator('canvas[data-testid="edit-canvas"]');
  await canvas.waitFor({ state: 'visible' });

  const pickBtn = page.locator('button[data-testid="point-colour-pick-btn"]');
  await pickBtn.waitFor({ state: 'visible' });
  await pickBtn.click();
  await page.waitForTimeout(50);

  const isEyedropperActive1 = await page.evaluate(() => {
    return document.querySelector('.canvas-viewport__container')?.classList.contains('is-eyedropper');
  });
  if (!isEyedropperActive1) {
    failures.push('Assertion 1: Clicking Pick did not activate eyedropper mode');
  }

  const box1 = await canvas.boundingBox();
  if (box1) {
    const clickX = box1.x + box1.width * 0.4;
    const clickY = box1.y + box1.height * 0.6;
    await page.mouse.click(clickX, clickY);
    await page.waitForTimeout(450);

    const st1 = await page.evaluate(() => ({
      calls: window.__STUB__.colourAtCalls,
      renders: window.__STUB__.renders,
      saved: window.__STUB__.lastSavedRecipe,
    }));

    if (st1.calls.length === 0) {
      failures.push('Assertion 1: Eyedropper click did not call colourAt');
    } else {
      const lastCall = st1.calls[st1.calls.length - 1];
      if (Math.abs(lastCall.x - 0.4) > 0.01 || Math.abs(lastCall.y - 0.6) > 0.01) {
        failures.push(`Assertion 1: Expected x=0.40, y=0.60 ± 0.01, got x=${lastCall.x.toFixed(3)}, y=${lastCall.y.toFixed(3)}`);
      }
    }
  } else {
    failures.push('Assertion 1: Canvas bounding box not found');
  }

  // -------------------------------------------------------------------------
  // 2. The same on orientation 6.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 2: The same on orientation 6...');
  await page.evaluate(() => {
    window.__STUB__.orientation = 6;
    window.__STUB__.toStoredOverride = null;
    window.__STUB__.colourAtCalls = [];
    window.__STUB__.colourAtResult = { hue: 195.0, chroma: 0.18, lightness: 0.52 };
    window.__STUB__.colourAtError = null;
    window.__STUB__.renders = [];
    window.__STUB__.lastSavedRecipe = null;
  });
  await pathInput.fill('/Volumes/Photos/orient6.jpg');
  await pathInput.press('Enter');
  await page.waitForTimeout(250);
  await blurAll();

  await ensureHslSectionOpen();

  const pickBtn2 = page.locator('button[data-testid="point-colour-pick-btn"]');
  await pickBtn2.click();
  await page.waitForTimeout(50);

  const box2 = await canvas.boundingBox();
  if (box2) {
    // Click at uDisp=0.7, vDisp=0.3
    // In orientation 6: uFrame = vDisp = 0.3, vFrame = 1.0 - uDisp = 0.3
    const clickX = box2.x + box2.width * 0.7;
    const clickY = box2.y + box2.height * 0.3;
    await page.mouse.click(clickX, clickY);
    await page.waitForTimeout(450);

    const st2 = await page.evaluate(() => ({
      calls: window.__STUB__.colourAtCalls,
      renders: window.__STUB__.renders,
      saved: window.__STUB__.lastSavedRecipe,
    }));

    if (st2.calls.length === 0) {
      failures.push('Assertion 2: Eyedropper click on orientation 6 did not call colourAt');
    } else {
      const lastCall = st2.calls[st2.calls.length - 1];
      if (Math.abs(lastCall.x - 0.3) > 0.01 || Math.abs(lastCall.y - 0.3) > 0.01) {
        failures.push(`Assertion 2: Orientation 6 coordinate mapping failed: expected x=0.30, y=0.30 ± 0.01; got x=${lastCall.x.toFixed(3)}, y=${lastCall.y.toFixed(3)}`);
      }
    }
  } else {
    failures.push('Assertion 2: Canvas bounding box not found');
  }

  // -------------------------------------------------------------------------
  // 3. The new point appears, selected, with its four sliders at their defaults,
  //    and is in the save payload.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 3: New point appears, selected, with four sliders at defaults, and in save payload...');
  const swatch0 = page.locator('button[data-testid="point-colour-swatch-0"]');
  await swatch0.waitFor({ state: 'visible' });
  const isSelected = await swatch0.evaluate((el) => el.classList.contains('active'));
  if (!isSelected) {
    failures.push('Assertion 3: Newly added point swatch is not selected (active)');
  }

  const hueInput = page.locator('input[data-testid="point-colour-hue-number"]');
  const satInput = page.locator('input[data-testid="point-colour-sat-number"]');
  const lumInput = page.locator('input[data-testid="point-colour-lum-number"]');
  const rangeInput = page.locator('input[data-testid="point-colour-range-number"]');

  const hueVal = await hueInput.inputValue();
  const satVal = await satInput.inputValue();
  const lumVal = await lumInput.inputValue();
  const rangeVal = await rangeInput.inputValue();

  if (hueVal !== '0') failures.push(`Assertion 3: Expected hue shift default 0, got ${hueVal}`);
  if (satVal !== '0') failures.push(`Assertion 3: Expected saturation default 0, got ${satVal}`);
  if (lumVal !== '0') failures.push(`Assertion 3: Expected luminance default 0, got ${lumVal}`);
  if (rangeVal !== '50') failures.push(`Assertion 3: Expected range default 50, got ${rangeVal}`);

  const savedRecipe3 = await page.evaluate(() => window.__STUB__.lastSavedRecipe);
  if (!savedRecipe3?.point_colours || savedRecipe3.point_colours.length === 0) {
    failures.push('Assertion 3: Point colour was not present in the save payload');
  } else {
    const pt = savedRecipe3.point_colours[0];
    if (Math.abs(pt.hue - 195.0) > 0.01 || Math.abs(pt.chroma - 0.18) > 0.01 || Math.abs(pt.lightness - 0.52) > 0.01) {
      failures.push(`Assertion 3: Saved point colour values mismatch: got hue=${pt.hue}, chroma=${pt.chroma}, lightness=${pt.lightness}`);
    }
  }

  // -------------------------------------------------------------------------
  // 4. Moving Saturation sends Drag renders carrying it; release sends Settle.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 4: Moving Saturation sends Drag renders carrying it; release sends Settle...');
  await page.evaluate(() => {
    window.__STUB__.renders = [];
  });

  const satSlider = page.locator('input[data-testid="point-colour-sat-slider"]');
  await satSlider.evaluate((el) => {
    const input = el;
    input.value = '42';
    input.dispatchEvent(new Event('input', { bubbles: true }));
  });
  await page.waitForTimeout(100);

  const dragRenders = await page.evaluate(() => {
    return window.__STUB__.renders.filter((r) => r.stage === 'Drag');
  });
  if (dragRenders.length === 0) {
    failures.push('Assertion 4: Moving Saturation did not send any Drag renders');
  } else {
    const lastDrag = dragRenders[dragRenders.length - 1];
    const dragSat = lastDrag.recipe.point_colours?.[0]?.saturation;
    if (dragSat !== 42) {
      failures.push(`Assertion 4: Drag render did not carry saturation 42, got ${dragSat}`);
    }
  }

  // Release slider
  await satSlider.evaluate((el) => {
    const input = el;
    input.dispatchEvent(new Event('change', { bubbles: true }));
  });
  await page.waitForTimeout(200);

  const settleRenders = await page.evaluate(() => {
    return window.__STUB__.renders.filter((r) => r.stage === 'Settle');
  });
  if (settleRenders.length === 0) {
    failures.push('Assertion 4: Releasing Saturation did not send a Settle render');
  } else {
    const lastSettle = settleRenders[settleRenders.length - 1];
    const settleSat = lastSettle.recipe.point_colours?.[0]?.saturation;
    if (settleSat !== 42) {
      failures.push(`Assertion 4: Settle render did not carry saturation 42, got ${settleSat}`);
    }
  }

  // -------------------------------------------------------------------------
  // 5. The history label is Point colour; Cmd-Z takes it back.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 5: History label is Point colour; Cmd-Z takes it back...');
  await page.waitForTimeout(400); // allow save debounce

  // Ensure History section is open to inspect items
  const historySec = page.locator('[data-testid="section-history"]');
  if (await historySec.isVisible()) {
    const isHistoryOpen = await historySec.evaluate((el) => el.classList.contains('is-open'));
    if (!isHistoryOpen) {
      await page.locator('button[data-testid="section-history-toggle"]').click();
      await page.waitForTimeout(100);
    }
  }

  const historyItems = page.locator('.history-item');
  const countHist = await historyItems.count();
  if (countHist > 0) {
    const firstLabel = (await historyItems.first().innerText()).trim();
    if (!firstLabel.toLowerCase().includes('point colour')) {
      failures.push(`Assertion 5: Expected history label to include "Point colour", got "${firstLabel}"`);
    }
  }

  // Press Cmd-Z to undo slider change
  await blurAll();
  await page.keyboard.press('Meta+z');
  await page.waitForTimeout(300);

  const satValUndone = await page.locator('input[data-testid="point-colour-sat-number"]').inputValue();
  if (satValUndone !== '0') {
    failures.push(`Assertion 5: Cmd-Z did not take back Saturation change; sat is ${satValUndone}`);
  }

  // -------------------------------------------------------------------------
  // 6. A stubbed refusal is shown and no point is added.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 6: Stubbed refusal is shown and no point is added...');
  const pointsCountBefore = await page.evaluate(() => window.__STUB__.lastSavedRecipe?.point_colours?.length ?? 1);

  await page.evaluate(() => {
    window.__STUB__.colourAtError = 'Chroma below 0.01: there is no colour there to select';
  });

  await ensureHslSectionOpen();
  await pickBtn.click();
  await page.waitForTimeout(50);

  if (box2) {
    await page.mouse.click(box2.x + box2.width * 0.5, box2.y + box2.height * 0.5);
    await page.waitForTimeout(200);

    const noticeEl = page.locator('[data-testid="edit-notice"]');
    const noticeVisible = await noticeEl.isVisible();
    if (!noticeVisible) {
      failures.push('Assertion 6: Notice toast was not shown on colourAt refusal');
    } else {
      const noticeText = await noticeEl.innerText();
      if (!noticeText.includes('Chroma below 0.01')) {
        failures.push(`Assertion 6: Notice text does not mention refusal reason: "${noticeText}"`);
      }
    }

    const pointsCountAfter = await page.evaluate(() => {
      const swatches = document.querySelectorAll('.point-colour-swatch');
      return swatches.length;
    });
    if (pointsCountAfter !== pointsCountBefore) {
      failures.push(`Assertion 6: Point count changed after refusal (before=${pointsCountBefore}, after=${pointsCountAfter})`);
    }
  }

  // -------------------------------------------------------------------------
  // 7. Delete removes it from the save payload.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 7: Delete removes it from the save payload...');
  const deleteBtn = page.locator('button[data-testid="point-colour-delete-btn"]');
  await deleteBtn.waitFor({ state: 'visible' });
  await deleteBtn.click();
  await page.waitForTimeout(450);

  const st7 = await page.evaluate(() => window.__STUB__.lastSavedRecipe);
  const remainingPoints = st7?.point_colours ?? [];
  if (remainingPoints.length > 0) {
    failures.push(`Assertion 7: Delete did not remove point from save payload; remaining: ${remainingPoints.length}`);
  }

  const deleteBtnVisible = await deleteBtn.isVisible();
  if (deleteBtnVisible) {
    failures.push('Assertion 7: Delete button is still visible when no point is selected');
  }

  // -------------------------------------------------------------------------
  // 8. At eight, Pick is disabled with the reason visible.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 8: At eight, Pick is disabled with the reason visible...');
  await page.evaluate(() => {
    window.__STUB__.colourAtError = null;
  });

  for (let i = 0; i < 8; i++) {
    await page.evaluate((idx) => {
      window.__STUB__.colourAtResult = { hue: idx * 40.0, chroma: 0.15, lightness: 0.5 };
    }, i);
    await pickBtn.click();
    await page.waitForTimeout(50);
    if (box2) {
      await page.mouse.click(box2.x + box2.width * 0.5, box2.y + box2.height * 0.5);
      await page.waitForTimeout(100);
    }
  }

  const isPickDisabled = await pickBtn.isDisabled();
  if (!isPickDisabled) {
    failures.push('Assertion 8: Pick button is not disabled when 8 points are present');
  }

  const limitMsg = page.locator('[data-testid="point-colour-limit-msg"]');
  const isLimitMsgVisible = await limitMsg.isVisible();
  if (!isLimitMsgVisible) {
    failures.push('Assertion 8: Limit message is not visible when 8 points are present');
  } else {
    const text = await limitMsg.innerText();
    if (!text.toLowerCase().includes('8')) {
      failures.push(`Assertion 8: Limit message does not mention 8: "${text}"`);
    }
  }

  // -------------------------------------------------------------------------
  // 9. The group is hidden while black & white is on, and the points are kept.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 9: Group is hidden while black & white is on, points kept...');
  await blurAll();
  await page.keyboard.press('v');
  await page.waitForTimeout(200);

  const panelInBw = await page.locator('[data-testid="point-colour-panel"]').isVisible();
  if (panelInBw) {
    failures.push('Assertion 9: Point colour panel is visible while black & white is active');
  }

  // Turn black & white off
  await blurAll();
  await page.keyboard.press('v');
  await page.waitForTimeout(200);

  const panelBack = await page.locator('[data-testid="point-colour-panel"]').isVisible();
  if (!panelBack) {
    failures.push('Assertion 9: Point colour panel is not restored when black & white is turned off');
  }

  const swatchesCount9 = await page.locator('.point-colour-swatch').count();
  if (swatchesCount9 !== 8) {
    failures.push(`Assertion 9: Expected 8 points kept after B&W toggle, found ${swatchesCount9}`);
  }

  // -------------------------------------------------------------------------
  // 10. W still sets white balance and adds no point.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 10: W still sets white balance and adds no point...');
  await page.evaluate(() => {
    window.__STUB__.whiteBalanceCalls = [];
    window.__STUB__.whiteBalanceResult = { temperature: 10, tint: -5, clamped: false };
    window.__STUB__.colourAtCalls = [];
  });

  await blurAll();
  await page.keyboard.press('w');
  await page.waitForTimeout(50);

  if (box2) {
    await page.mouse.click(box2.x + box2.width * 0.4, box2.y + box2.height * 0.6);
    await page.waitForTimeout(450);

    const st10 = await page.evaluate(() => ({
      wbCalls: window.__STUB__.whiteBalanceCalls.length,
      colCalls: window.__STUB__.colourAtCalls.length,
      saved: window.__STUB__.lastSavedRecipe,
    }));

    if (st10.wbCalls === 0) {
      failures.push('Assertion 10: Pressing W did not invoke whiteBalanceAt');
    }
    if (st10.colCalls > 0) {
      failures.push(`Assertion 10: Pressing W called colourAt (${st10.colCalls} times)`);
    }
    if (st10.saved?.point_colours?.length !== 8) {
      failures.push(`Assertion 10: Point colours count changed from 8 to ${st10.saved?.point_colours?.length}`);
    }
    if (st10.saved?.temperature !== 10 || st10.saved?.tint !== -5) {
      failures.push(`Assertion 10: White balance was not updated (temp=${st10.saved?.temperature}, tint=${st10.saved?.tint})`);
    }
  }

  // -------------------------------------------------------------------------
  // 11. Every new control is at least 40 px. Screenshot to layout-proof/point-colour.png.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 11: Every new control is at least 40 px. Screenshot to layout-proof/point-colour.png...');
  const controlsToCheck = [
    'point-colour-pick-btn',
    'point-colour-swatch-0',
    'point-colour-delete-btn',
  ];

  for (const id of controlsToCheck) {
    const loc = page.locator(`[data-testid="${id}"]`);
    if (await loc.isVisible()) {
      const box = await loc.boundingBox();
      if (!box || box.width < 39.5 || box.height < 39.5) {
        failures.push(`Assertion 11: Control ${id} touch target is under 40px (${box?.width?.toFixed(1)}x${box?.height?.toFixed(1)})`);
      }
    } else {
      failures.push(`Assertion 11: Control ${id} is not visible to check size`);
    }
  }

  const sliderIds = [
    'point-colour-hue-slider',
    'point-colour-sat-slider',
    'point-colour-lum-slider',
    'point-colour-range-slider',
  ];
  for (const id of sliderIds) {
    const loc = page.locator(`[data-testid="${id}"]`);
    if (await loc.isVisible()) {
      const box = await loc.boundingBox();
      if (!box || box.height < 39.5) {
        failures.push(`Assertion 11: Slider ${id} height is under 40px (${box?.height?.toFixed(1)})`);
      }
    } else {
      failures.push(`Assertion 11: Slider ${id} is not visible to check size`);
    }
  }

  const proofPath = join(OUT, 'point-colour.png');
  await page.screenshot({ path: proofPath, fullPage: true });
  console.log(`  Proof screenshot saved to ${proofPath}`);

} catch (err) {
  failures.push(`Unexpected error during check:point execution: ${err.message}`);
} finally {
  await browser.close();
  await server.close();
}

console.log('\n--- check:point Summary ---');
console.log(`Assertions executed: ${assertionsRun}/11`);
if (failures.length === 0) {
  console.log('ALL 11 ASSERTIONS PASSED');
  process.exit(0);
} else {
  console.error(`FAILURES (${failures.length}):`);
  for (const f of failures) {
    console.error(`  - ${f}`);
  }
  process.exit(1);
}
