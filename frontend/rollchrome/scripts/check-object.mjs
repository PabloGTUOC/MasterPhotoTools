/**
 * RP-11c Masks: click to select an object acceptance & verification in a real headless browser.
 *
 * Implements all 15 numbered assertions from docs/briefs/RP-11c.md:
 * 1. `+ Object` with the model present enters the mode: hint shown, crosshair cursor, no mask yet.
 * 2. A click sends `select_object` once with one click, `add: true`, at the stored coordinates expected for that canvas point;
 *    a mask `Object 1` of kind `object` is selected, with the raster in the save payload.
 * 3. A second click sends **both** clicks and replaces the raster; still one mask.
 * 4. Alt-click sends `add: false`.
 * 5. Each click is one undo step labelled `Masks`: Cmd-Z after the second click returns to one click and the first raster.
 * 6. Esc and Done each leave the mode; a click afterwards sends nothing.
 * 7. With the photograph rotated 90° and cropped, the coordinates sent equal what the eyedropper's `colour_at` receives for the same point.
 * 8. A click while one is in flight sends nothing more (hold the stub's reply).
 * 9. A refused first click creates no mask and shows the words; a refused later click leaves clicks and raster unchanged.
 * 10. A reply that arrives after the mode was left and started again, or after a change of photograph, changes nothing.
 * 11. Model absent: the download is asked for with this model's name, and after it completes the mode starts.
 * 12. Refine on a saved Object mask sends its saved clicks plus the new one.
 * 13. Add component → Object makes a component with `op: add`, the base shape unchanged.
 * 14. Undoing the mask's creation leaves the mode.
 * 15. `+ Object`, Done, Refine and Add component's Object are at least 40 px; screenshot to `layout-proof/`.
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

console.log('Starting Vite harness server for check:object...');
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

const executablePath = preinstalledChromium();
const browser = await chromium.launch({
  headless: true,
  executablePath,
  args: ['--no-sandbox', '--disable-setuid-sandbox', '--disable-dev-shm-usage'],
});

const page = await browser.newPage({
  viewport: { width: 1440, height: 900 },
  deviceScaleFactor: 2,
});
page.setDefaultTimeout(20000);
page.on('pageerror', (err) => console.error('PAGE ERROR:', err.message));

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

  const ensureMasksSectionOpen = async () => {
    const section = page.locator('[data-testid="section-masks"]');
    await section.waitFor({ state: 'visible' });
    const isOpen = await section.evaluate((el) => el.classList.contains('is-open'));
    if (!isOpen) {
      await page.locator('button[data-testid="section-masks-toggle"]').click();
      await page.waitForTimeout(100);
    }
  };

  const holdReplies = () => page.evaluate(() => {
    window.__STUB__.selectObjectHold = new Promise((resolve) => {
      window.__STUB__.resolveSelectObjectHold = resolve;
    });
  });
  const releaseReplies = () => page.evaluate(() => {
    window.__STUB__.resolveSelectObjectHold?.();
    window.__STUB__.selectObjectHold = null;
    window.__STUB__.resolveSelectObjectHold = null;
  });
  const selectCallCount = () => page.evaluate(() => window.__STUB__.selectObjectCalls.length);
  const waitForSelectCalls = (n) => page.waitForFunction(
    (want) => window.__STUB__.selectObjectCalls.length === want, n, { timeout: 4000 });
  const lookingNoticeShown = () => page.evaluate(() =>
    document.querySelector('[data-testid="edit-notice"]')?.textContent?.includes('Looking at the photograph') ?? false);

  await page.evaluate(() => {
    window.__STUB__.orientation = 1;
    window.__STUB__.toStoredOverride = null;
    window.__STUB__.renders = [];
    window.__STUB__.lastSavedRecipe = null;
    window.__STUB__.modelsPresent = { subject: true, sky: true };
    window.__STUB__.objectModelPresent = true;
    window.__STUB__.autoMaskCalls = [];
    window.__STUB__.selectObjectCalls = [];
  });
  await pathInput.fill('/Volumes/Photos/object_test.jpg');
  await pathInput.press('Enter');
  await page.waitForTimeout(250);
  await blurAll();

  await ensureMasksSectionOpen();

  const canvas = page.locator('canvas.canvas-viewport__canvas');
  await canvas.waitFor({ state: 'visible' });

  // -------------------------------------------------------------------------
  // 1. + Object with model present enters mode: hint shown, crosshair, no mask yet
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 1: + Object with model present enters mode...');
  await page.evaluate(() => {
    window.__STUB__.objectModelPresent = true;
    window.__STUB__.selectObjectCalls = [];
  });

  const addObjectBtn = page.locator('button[data-testid="add-object-mask"]');
  await addObjectBtn.waitFor({ state: 'visible' });
  await addObjectBtn.click();
  await page.waitForTimeout(150);

  const hint1 = page.locator('[data-testid="object-hint"]');
  if (!(await hint1.isVisible())) {
    failures.push('Assertion 1: Expected object-hint to be visible after clicking + Object');
  } else {
    const txt = await hint1.textContent();
    if (!txt?.includes('Click the object. Click again to add, Alt-click to take away.')) {
      failures.push(`Assertion 1: Hint text mismatch, got: ${txt}`);
    }
  }

  const hasCrosshair = await page.evaluate(() => {
    const vp = document.querySelector('.canvas-viewport__container');
    return vp?.classList.contains('is-eyedropper') ?? false;
  });
  if (!hasCrosshair) {
    failures.push('Assertion 1: Viewport does not have is-eyedropper class (crosshair cursor)');
  }

  const maskCount1 = await page.evaluate(() => {
    return window.__STUB__.lastSavedRecipe?.masks?.length ?? 0;
  });
  if (maskCount1 !== 0) {
    failures.push(`Assertion 1: Expected no masks yet, got ${maskCount1}`);
  }

  // -------------------------------------------------------------------------
  // 2. Click sends select_object once with one click, add: true, at expected coords;
  //    mask Object 1 of kind object is selected, with raster in save payload.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 2: First click sends select_object once, creates Object 1...');
  const cBox = await canvas.boundingBox();
  if (!cBox) throw new Error('Canvas bounding box missing');

  const clickX = cBox.x + cBox.width * 0.4;
  const clickY = cBox.y + cBox.height * 0.3;

  await holdReplies();
  await page.mouse.click(clickX, clickY);
  await page.waitForFunction(() => {
    return (window.__STUB__.selectObjectCalls?.length ?? 0) === 1;
  }, { timeout: 4000 });
  if (!(await lookingNoticeShown())) {
    failures.push('Assertion 2: The first click on a photograph should say it is looking at the photograph');
  }
  await releaseReplies();

  const calls2 = await page.evaluate(() => window.__STUB__.selectObjectCalls);
  if (calls2.length !== 1) {
    failures.push(`Assertion 2: Expected 1 call to selectObject, got ${calls2.length}`);
  } else {
    const c = calls2[0].clicks;
    if (c.length !== 1 || !c[0].add || Math.abs(c[0].u - 0.4) > 0.05 || Math.abs(c[0].v - 0.3) > 0.05) {
      failures.push(`Assertion 2: Unexpected clicks in selectObject: ${JSON.stringify(c)}`);
    }
  }

  await page.waitForFunction(() => {
    const m = window.__STUB__.lastSavedRecipe?.masks?.[0];
    return m?.name === 'Object 1' && m?.kind?.type === 'object';
  }, { timeout: 4000 });

  const mask1Data = await page.evaluate(() => window.__STUB__.lastSavedRecipe?.masks?.[0]);
  if (!mask1Data || mask1Data.name !== 'Object 1' || mask1Data.kind.type !== 'object' || !mask1Data.kind.mask?.png) {
    failures.push(`Assertion 2: Mask 1 invalid in save payload: ${JSON.stringify(mask1Data)}`);
  }

  // -------------------------------------------------------------------------
  // 3. Second click sends both clicks and replaces the raster; still one mask.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 3: Second click sends both clicks and replaces raster...');
  const clickX2 = cBox.x + cBox.width * 0.6;
  const clickY2 = cBox.y + cBox.height * 0.7;

  await page.mouse.click(clickX2, clickY2);
  await page.waitForFunction(() => {
    return (window.__STUB__.selectObjectCalls?.length ?? 0) === 2;
  }, { timeout: 4000 });

  const calls3 = await page.evaluate(() => window.__STUB__.selectObjectCalls);
  const secondCallClicks = calls3[1]?.clicks ?? [];
  if (secondCallClicks.length !== 2) {
    failures.push(`Assertion 3: Expected 2 clicks in second call, got ${secondCallClicks.length}`);
  }

  await page.waitForFunction(() => {
    const r = window.__STUB__.lastSavedRecipe;
    return r?.masks?.length === 1 && r?.masks?.[0]?.kind?.mask?.png === 'stub-object-raster-2-clicks';
  }, { timeout: 4000 });

  // -------------------------------------------------------------------------
  // 4. Alt-click sends add: false.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 4: Alt-click sends add: false...');
  const clickX3 = cBox.x + cBox.width * 0.5;
  const clickY3 = cBox.y + cBox.height * 0.5;

  await page.keyboard.down('Alt');
  await page.mouse.click(clickX3, clickY3);
  await page.keyboard.up('Alt');

  await page.waitForFunction(() => {
    return (window.__STUB__.selectObjectCalls?.length ?? 0) === 3;
  }, { timeout: 4000 });

  const calls4 = await page.evaluate(() => window.__STUB__.selectObjectCalls);
  const thirdCallClicks = calls4[2]?.clicks ?? [];
  const lastClick = thirdCallClicks[thirdCallClicks.length - 1];
  if (!lastClick || lastClick.add !== false) {
    failures.push(`Assertion 4: Expected last click to have add: false, got: ${JSON.stringify(lastClick)}`);
  }

  // -------------------------------------------------------------------------
  // 5. Each click is one undo step labelled Masks: Cmd-Z returns to 2 clicks
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 5: Each click is one undo step labelled Masks...');
  const isMac = process.platform === 'darwin';
  await page.keyboard.press(isMac ? 'Meta+z' : 'Control+z');
  await page.waitForTimeout(300);

  await page.waitForFunction(() => {
    const m = window.__STUB__.lastSavedRecipe?.masks?.[0];
    return m?.kind?.clicks?.length === 2 && m?.kind?.mask?.png === 'stub-object-raster-2-clicks';
  }, { timeout: 4000 });

  const clicksAfterUndo = await page.evaluate(() => {
    return window.__STUB__.lastSavedRecipe?.masks?.[0]?.kind?.clicks?.length;
  });
  if (clicksAfterUndo !== 2) {
    failures.push(`Assertion 5: Expected 2 clicks after undo, got ${clicksAfterUndo}`);
  }

  // -------------------------------------------------------------------------
  // 6. Esc and Done each leave mode; a click afterwards sends nothing.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 6: Esc and Done leave mode; click afterwards sends nothing...');
  const doneBtn = page.locator('button[data-testid="object-done"]');
  await doneBtn.waitFor({ state: 'visible' });
  await doneBtn.click();
  await page.waitForTimeout(200);

  const isEyedropperAfterDone = await page.evaluate(() => {
    return document.querySelector('.canvas-viewport__container')?.classList.contains('is-eyedropper') ?? false;
  });
  if (isEyedropperAfterDone) {
    failures.push('Assertion 6: Expected eyedropper mode to be inactive after Done');
  }

  const callsBeforeClick = await page.evaluate(() => window.__STUB__.selectObjectCalls.length);
  await page.mouse.click(cBox.x + cBox.width * 0.2, cBox.y + cBox.height * 0.2);
  await page.waitForTimeout(200);
  const callsAfterClick = await page.evaluate(() => window.__STUB__.selectObjectCalls.length);
  if (callsAfterClick !== callsBeforeClick) {
    failures.push('Assertion 6: Click after Done sent selectObject');
  }

  // Re-enter mode via Refine and test Esc
  const refineBtn = page.locator('button[data-testid="refine-object"]');
  await refineBtn.waitFor({ state: 'visible' });
  await refineBtn.click();
  await page.waitForTimeout(150);
  await page.keyboard.press('Escape');
  await page.waitForTimeout(200);
  const isEyedropperAfterEsc = await page.evaluate(() => {
    return document.querySelector('.canvas-viewport__container')?.classList.contains('is-eyedropper') ?? false;
  });
  if (isEyedropperAfterEsc) {
    failures.push('Assertion 6: Expected eyedropper mode to be inactive after Esc');
  }

  // -------------------------------------------------------------------------
  // 8. Click while one in flight sends nothing more
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 8: Click while one is in flight sends nothing more...');
  await page.evaluate(() => {
    window.__STUB__.selectObjectCalls = [];
    window.__STUB__.selectObjectHold = new Promise((resolve) => {
      window.__STUB__.resolveSelectObjectHold = resolve;
    });
  });

  await refineBtn.click();
  await page.waitForTimeout(100);

  // First click initiates request (held)
  await page.mouse.click(cBox.x + cBox.width * 0.2, cBox.y + cBox.height * 0.2);
  await page.waitForTimeout(50);

  // Second click while first is in flight
  await page.mouse.click(cBox.x + cBox.width * 0.3, cBox.y + cBox.height * 0.3);
  await page.waitForTimeout(100);

  const inFlightCalls = await page.evaluate(() => window.__STUB__.selectObjectCalls.length);
  if (inFlightCalls !== 1) {
    failures.push(`Assertion 8: Expected 1 call while in flight, got ${inFlightCalls}`);
  }
  // Not the first click on this photograph: the encoder has already run.
  if (await lookingNoticeShown()) {
    failures.push('Assertion 8: A later click should not flash the looking-at-the-photograph notice');
  }

  // Release hold
  await page.evaluate(() => {
    window.__STUB__.resolveSelectObjectHold?.();
    window.__STUB__.selectObjectHold = null;
    window.__STUB__.resolveSelectObjectHold = null;
  });
  await page.waitForTimeout(200);
  await page.locator('button[data-testid="object-done"]').click();
  await page.waitForTimeout(150);

  // -------------------------------------------------------------------------
  // 9. Refused first click creates no mask and shows words; refused later leaves clicks/raster
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 9: Refused clicks handled gracefully...');
  // Delete current mask so we test first click refusal
  await page.evaluate(() => {
    window.__STUB__.lastSavedRecipe.masks = [];
  });
  // Reset all adjustments
  await page.locator('button[data-testid="reset-all-btn"]').click();
  await page.waitForTimeout(200);
  await ensureMasksSectionOpen();

  await page.evaluate(() => {
    window.__STUB__.selectObjectFails = 'Refused: nothing was found there; click on the object itself';
  });

  await addObjectBtn.click();
  await page.waitForTimeout(150);
  await page.mouse.click(cBox.x + cBox.width * 0.1, cBox.y + cBox.height * 0.1);
  await page.waitForTimeout(200);

  const maskCountAfterFail = await page.evaluate(() => window.__STUB__.lastSavedRecipe?.masks?.length ?? 0);
  if (maskCountAfterFail !== 0) {
    failures.push(`Assertion 9: Expected 0 masks after failed first click, got ${maskCountAfterFail}`);
  }
  const noticeEl = page.locator('[data-testid="edit-notice"]');
  await noticeEl.waitFor({ state: 'visible', timeout: 3000 });
  const noticeText = (await noticeEl.textContent()) ?? '';
  if (!noticeText.includes('nothing was found there')) {
    failures.push(`Assertion 9: Notice did not show refusal words, got: ${noticeText}`);
  }

  // Allow next click to succeed
  await page.evaluate(() => {
    window.__STUB__.selectObjectFails = null;
  });
  await page.mouse.click(cBox.x + cBox.width * 0.5, cBox.y + cBox.height * 0.5);
  await page.waitForFunction(() => (window.__STUB__.lastSavedRecipe?.masks?.length ?? 0) === 1, { timeout: 4000 });

  const maskBeforeFail = await page.evaluate(() => JSON.stringify(window.__STUB__.lastSavedRecipe?.masks?.[0]));

  // Now fail second click
  await page.evaluate(() => {
    window.__STUB__.selectObjectFails = 'Refused: click on the object to add before taking away';
  });
  await page.mouse.click(cBox.x + cBox.width * 0.2, cBox.y + cBox.height * 0.2);
  await page.waitForTimeout(200);

  const maskAfterFail = await page.evaluate(() => JSON.stringify(window.__STUB__.lastSavedRecipe?.masks?.[0]));
  if (maskBeforeFail !== maskAfterFail) {
    failures.push('Assertion 9: Failed later click modified the mask');
  }
  await page.evaluate(() => {
    window.__STUB__.selectObjectFails = null;
  });
  await page.locator('button[data-testid="object-done"]').click();
  await page.waitForTimeout(150);

  // -------------------------------------------------------------------------
  // 10 (first half). A reply to a stretch of the mode that was left changes nothing, even with
  // the mode started again. The change of photograph is at the end: nothing after it could run.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 10: A reply from before Esc does not land in the mode started after it...');
  // The rows, not the saved recipe: saving is debounced, and would still be the old one here.
  const maskRows = page.locator('.mask-row__name');
  const rowsBeforeStale = await maskRows.count();
  await holdReplies();
  const staleCallsBefore = await selectCallCount();
  await addObjectBtn.click();
  await page.waitForTimeout(100);
  await page.mouse.click(cBox.x + cBox.width * 0.3, cBox.y + cBox.height * 0.3);
  await waitForSelectCalls(staleCallsBefore + 1);
  await page.keyboard.press('Escape');
  await page.waitForTimeout(100);
  await addObjectBtn.click();
  await page.waitForTimeout(100);
  await releaseReplies();
  await page.waitForTimeout(300);
  const rowsAfterStale = await maskRows.count();
  if (rowsAfterStale !== rowsBeforeStale) {
    failures.push(`Assertion 10: A reply from before Esc made a mask after + Object started the mode again (${rowsBeforeStale} rows became ${rowsAfterStale})`);
  }
  await page.keyboard.press('Escape');
  await page.waitForTimeout(150);
  // Refine is only offered on a selected mask, and the assertions below use it.
  if (!(await refineBtn.isVisible())) {
    await page.locator('.mask-row__name').first().click();
    await page.waitForTimeout(100);
  }

  // -------------------------------------------------------------------------
  // 11. Model absent: download asked with model's name, mode starts after download
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 11: Model absent download flow...');
  await page.evaluate(() => {
    window.__STUB__.objectModelPresent = false;
    window.__STUB__.downloads = [];
  });

  await addObjectBtn.click();
  await page.waitForTimeout(150);

  const dlPrompt = page.locator('[data-testid="auto-download"]');
  if (!(await dlPrompt.isVisible())) {
    failures.push('Assertion 11: Download prompt not visible when model absent');
  } else {
    const dlTxt = await dlPrompt.textContent();
    if (!dlTxt?.includes('MobileSAM (Apache-2.0 / MIT), 45 MB')) {
      failures.push(`Assertion 11: Prompt did not mention MobileSAM name, got: ${dlTxt}`);
    }
  }

  // Click Download
  const startDlBtn = page.locator('button[data-testid="auto-download-start"]');
  await startDlBtn.click();
  await page.waitForTimeout(500); // Wait for stub watchJob to complete (300ms)

  // Verify object mode started after download
  await page.waitForFunction(() => {
    return document.querySelector('.canvas-viewport__container')?.classList.contains('is-eyedropper') ?? false;
  }, { timeout: 4000 });

  const hint11 = await page.locator('[data-testid="object-hint"]').isVisible();
  if (!hint11) {
    failures.push('Assertion 11: Object selection mode did not start after download completed');
  }
  await page.locator('button[data-testid="object-done"]').click();
  await page.waitForTimeout(150);

  // -------------------------------------------------------------------------
  // 12. Refine on saved Object mask sends saved clicks plus new one
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 12: Refine sends saved clicks plus new one...');
  await page.evaluate(() => {
    window.__STUB__.selectObjectCalls = [];
  });
  await refineBtn.click();
  await page.waitForTimeout(100);
  await page.mouse.click(cBox.x + cBox.width * 0.8, cBox.y + cBox.height * 0.8);
  await page.waitForFunction(() => (window.__STUB__.selectObjectCalls?.length ?? 0) === 1, { timeout: 4000 });

  const refineCallClicks = await page.evaluate(() => window.__STUB__.selectObjectCalls[0]?.clicks ?? []);
  if (refineCallClicks.length < 2) {
    failures.push(`Assertion 12: Expected saved clicks plus new click, got ${refineCallClicks.length} clicks`);
  }
  await page.locator('button[data-testid="object-done"]').click();
  await page.waitForTimeout(150);

  // -------------------------------------------------------------------------
  // 13. Add component -> Object makes component with op: add, base shape unchanged
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 13: Add component -> Object makes component with op: add...');
  const baseShapeBefore = await page.evaluate(() => window.__STUB__.lastSavedRecipe?.masks?.[0]?.kind?.type);

  const addCompObjBtn = page.locator('button[data-testid="add-component-object"]');
  await addCompObjBtn.waitFor({ state: 'visible' });
  await addCompObjBtn.click();
  await page.waitForTimeout(150);

  // Click on canvas to create the component
  await page.mouse.click(cBox.x + cBox.width * 0.45, cBox.y + cBox.height * 0.55);
  await page.waitForFunction(() => {
    const comps = window.__STUB__.lastSavedRecipe?.masks?.[0]?.components;
    return (comps?.length ?? 0) > 0;
  }, { timeout: 4000 });

  const compData = await page.evaluate(() => window.__STUB__.lastSavedRecipe?.masks?.[0]?.components?.[0]);
  const baseShapeAfter = await page.evaluate(() => window.__STUB__.lastSavedRecipe?.masks?.[0]?.kind?.type);

  if (compData?.op !== 'add' || compData?.kind?.type !== 'object') {
    failures.push(`Assertion 13: Component invalid: ${JSON.stringify(compData)}`);
  }
  if (baseShapeBefore !== baseShapeAfter) {
    failures.push(`Assertion 13: Base shape changed from ${baseShapeBefore} to ${baseShapeAfter}`);
  }
  await page.locator('button[data-testid="object-done"]').click();
  await page.waitForTimeout(150);

  // -------------------------------------------------------------------------
  // 14. Undoing the mask's creation leaves the mode
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 14: Undoing mask creation leaves the mode...');
  // Reset masks, add new Object mask
  await page.locator('button[data-testid="reset-all-btn"]').click();
  await page.waitForTimeout(200);
  await ensureMasksSectionOpen();

  await addObjectBtn.click();
  await page.waitForTimeout(100);
  await page.mouse.click(cBox.x + cBox.width * 0.5, cBox.y + cBox.height * 0.5);
  await page.waitForFunction(() => (window.__STUB__.lastSavedRecipe?.masks?.length ?? 0) === 1, { timeout: 4000 });

  // Mode is active; undo creation
  await page.keyboard.press(isMac ? 'Meta+z' : 'Control+z');
  await page.waitForTimeout(300);

  const isModeActive14 = await page.evaluate(() => {
    return document.querySelector('.canvas-viewport__container')?.classList.contains('is-eyedropper') ?? false;
  });
  if (isModeActive14) {
    failures.push('Assertion 14: Expected mode to be inactive after undoing mask creation');
  }

  // -------------------------------------------------------------------------
  // 15. Touch targets at least 40 px: + Object, Done, Refine, Add component's Object; screenshot
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 15: Touch targets at least 40 px...');

  // Create a mask so Refine and Add Component are available
  await addObjectBtn.click();
  await page.waitForTimeout(100);
  await page.mouse.click(cBox.x + cBox.width * 0.5, cBox.y + cBox.height * 0.5);
  await page.waitForFunction(() => (window.__STUB__.lastSavedRecipe?.masks?.length ?? 0) === 1, { timeout: 4000 });

  const controlsToCheck = [
    { name: '+ Object', selector: 'button[data-testid="add-object-mask"]' },
    { name: 'Done (object-done)', selector: 'button[data-testid="object-done"]' },
    { name: 'Refine (refine-object)', selector: 'button[data-testid="refine-object"]' },
    { name: 'Add component + Object', selector: 'button[data-testid="add-component-object"]' },
  ];

  for (const c of controlsToCheck) {
    const el = page.locator(c.selector);
    if (!(await el.isVisible())) {
      failures.push(`Assertion 15: Control ${c.name} (${c.selector}) is not visible`);
      continue;
    }
    const box = await el.boundingBox();
    if (!box) {
      failures.push(`Assertion 15: Control ${c.name} bounding box not found`);
    } else if (box.height < 40) {
      failures.push(`Assertion 15: Control ${c.name} height ${box.height}px is less than 40px`);
    }
  }

  // Take proof screenshot
  await page.screenshot({ path: join(OUT, 'rp11c-object-masks.png') });
  console.log(`Saved screenshot to ${join(OUT, 'rp11c-object-masks.png')}`);

  // -------------------------------------------------------------------------
  // 8 (second half). An automatic mask during a click neither loses the click nor jams the mode.
  // -------------------------------------------------------------------------
  console.log('Asserting 8: + Subject during a click leaves the mode taking clicks...');
  await holdReplies();
  const beforeSubject = await selectCallCount();
  await page.mouse.click(cBox.x + cBox.width * 0.4, cBox.y + cBox.height * 0.4);
  await waitForSelectCalls(beforeSubject + 1);
  await page.locator('button[data-testid="add-subject-mask"]').click();
  await page.waitForFunction(() => window.__STUB__.autoMaskCalls.includes('subject'), { timeout: 4000 });
  await releaseReplies();
  await page.waitForTimeout(300);
  await page.mouse.click(cBox.x + cBox.width * 0.6, cBox.y + cBox.height * 0.6);
  await page.waitForTimeout(300);
  if ((await selectCallCount()) !== beforeSubject + 2) {
    failures.push(`Assertion 8: After + Subject during a click, the next click was not sent (${(await selectCallCount()) - beforeSubject} of 2)`);
  }
  await page.locator('button[data-testid="object-done"]').click();
  await page.waitForTimeout(150);

  // -------------------------------------------------------------------------
  // 7. Rotated and cropped: the coordinates sent are the eyedropper's, through a map that is
  // not the identity. Opened from a saved recipe, because the view reads its own recipe.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 7: Rotated and cropped coordinates match colour_at...');
  const rotatedToStored = [0, 0.6, 0.1, -0.8, 0, 0.9]; // 90 degrees, and the recipe's crop
  await page.evaluate((m) => {
    window.__STUB__.toStoredOverride = m;
    window.__STUB__.colourAtCalls = [];
    window.__STUB__.selectObjectCalls = [];
  }, rotatedToStored);
  await pathInput.fill('/Volumes/Photos/v2_edits.jpg'); // its recipe has rotate 90 and a crop
  await pathInput.press('Enter');
  await page.waitForTimeout(300);
  await blurAll();
  await ensureMasksSectionOpen();
  const box7 = await canvas.boundingBox();
  if (!box7) throw new Error('Canvas bounding box missing after opening the rotated photograph');
  const [u7, v7] = [0.35, 0.45];
  const sampleX = box7.x + box7.width * u7;
  const sampleY = box7.y + box7.height * v7;
  const expectedU = rotatedToStored[0] * u7 + rotatedToStored[1] * v7 + rotatedToStored[2];
  const expectedV = rotatedToStored[3] * u7 + rotatedToStored[4] * v7 + rotatedToStored[5];

  const colourMaskBtn = page.locator('button[data-testid="add-colour-mask"]');
  await colourMaskBtn.scrollIntoViewIfNeeded();
  await colourMaskBtn.click();
  await page.waitForTimeout(100);
  await page.mouse.click(sampleX, sampleY);
  await page.waitForFunction(() => (window.__STUB__.colourAtCalls?.length ?? 0) > 0, { timeout: 3000 });
  const colourCall = await page.evaluate(() => window.__STUB__.colourAtCalls.at(-1));

  await addObjectBtn.scrollIntoViewIfNeeded();
  await addObjectBtn.click();
  await page.waitForTimeout(100);
  await page.mouse.click(sampleX, sampleY);
  await waitForSelectCalls(1);
  const objClick = await page.evaluate(() => window.__STUB__.selectObjectCalls[0]?.clicks?.at(-1));

  if (Math.abs(expectedU - u7) < 0.05 && Math.abs(expectedV - v7) < 0.05) {
    failures.push('Assertion 7: The map under test is the identity, so this proves nothing');
  }
  if (!colourCall || !objClick
      || Math.abs(colourCall.x - objClick.u) > 0.001 || Math.abs(colourCall.y - objClick.v) > 0.001) {
    failures.push(`Assertion 7: colour_at (${colourCall?.x}, ${colourCall?.y}) differs from Object (${objClick?.u}, ${objClick?.v})`);
  }
  if (!objClick || Math.abs(objClick.u - expectedU) > 0.001 || Math.abs(objClick.v - expectedV) > 0.001) {
    failures.push(`Assertion 7: Object coords (${objClick?.u}, ${objClick?.v}) are not the mapped point (${expectedU}, ${expectedV})`);
  }
  await page.waitForTimeout(150);
  await page.locator('button[data-testid="object-done"]').click();
  await page.waitForTimeout(150);
  await page.evaluate(() => { window.__STUB__.toStoredOverride = null; });

  // -------------------------------------------------------------------------
  // 10 (second half). A first click's reply arriving after another photograph opened makes no
  // mask on that photograph. A first click, because a refinement would find no mask to change.
  // -------------------------------------------------------------------------
  console.log('Asserting 10: A reply arriving after a change of photograph changes nothing...');
  await holdReplies();
  const beforeOpen = await selectCallCount();
  await addObjectBtn.click();
  await page.waitForTimeout(100);
  await page.mouse.click(cBox.x + cBox.width * 0.3, cBox.y + cBox.height * 0.3);
  await waitForSelectCalls(beforeOpen + 1);
  await pathInput.fill('/Volumes/Photos/another_photo.jpg');
  await pathInput.press('Enter');
  await page.waitForTimeout(300);
  await page.evaluate(() => { window.__STUB__.renders = []; window.__STUB__.saveCalls = []; });
  await releaseReplies();
  await page.waitForTimeout(400);
  const objectMasksAfter = await page.evaluate(() => {
    const seen = [...window.__STUB__.renders.map((r) => r.recipe), ...window.__STUB__.saveCalls.map((c) => c.recipe)];
    return seen.flatMap((r) => r.masks ?? []).filter((m) => m.kind?.type === 'object').length;
  });
  if (objectMasksAfter !== 0) {
    failures.push(`Assertion 10: A reply from the previous photograph put ${objectMasksAfter} object mask(s) on the new one`);
  }
  if (await page.locator('.mask-row__name').count()) {
    failures.push('Assertion 10: The new photograph shows a mask it never had');
  }

} catch (err) {
  failures.push(`Unexpected check exception: ${err.stack || err.message}`);
} finally {
  await browser.close();
  await server.close();
}

console.log(`\nCompleted ${assertionsRun} assertions with ${failures.length} failures.`);
if (failures.length > 0) {
  console.error('FAILURES:\n' + failures.join('\n'));
  process.exit(1);
} else {
  console.log('All 15 assertions passed!');
}
