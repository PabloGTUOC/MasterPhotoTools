/**
 * RP-5 History, Undo and Snapshots acceptance & verification in a real headless browser.
 *
 * Asserts all 16 requirements as numbered in the brief:
 * 1. Two slider commits, then Cmd-Z: the first's value is on screen, a render with it was requested, and a save with it was sent. Cmd-Shift-Z brings the second back.
 * 2. Twenty input events and one release make exactly one step.
 * 3. A new change after an undo removes the redo.
 * 4. Cmd-Z with nothing to undo does nothing and sends nothing.
 * 5. The list shows the labels, newest first, and the current one is marked; clicking an older one restores it.
 * 6. A preset, a crop applied, a mask added, Auto, the eyedropper and black & white are each one step, each with its proper label, and each undone.
 * 7. Undoing the creation of the selected mask deselects it.
 * 8. With a text field focused, Cmd-Z is not intercepted.
 * 9. In crop mode, Cmd-Z does nothing.
 * 10. Opening another photograph empties the list.
 * 11. A snapshot appears in the save payload; restoring it is one step; undoing puts back what was there and leaves the snapshot in the list; deleting removes it from the next save.
 * 12. An empty and a duplicate name are refused with a visible message.
 * 13. Pasting settings from a photograph with snapshots brings none.
 * 14. On a read-only photograph, undo works and no save is sent.
 * 15. EditHistory alone: 150 commits keep 100; a set of large recipes stays under maxBytes, oldest gone, the current kept.
 * 16. Every new control is at least 40 px. Screenshot to layout-proof/.
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

console.log('Starting Vite harness server for check:history...');
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

  const pressUndo = async () => {
    await blurAll();
    await page.keyboard.press('Meta+z');
  };

  const pressRedo = async () => {
    await blurAll();
    await page.keyboard.press('Meta+Shift+z');
  };

  // Helper to set a slider value via its number field and fire change
  const setSliderValue = async (testId, value) => {
    const input = page.locator(`input[data-testid="${testId}-number"]`);
    await input.fill(String(value));
    await input.dispatchEvent('change');
    await blurAll();
  };

  // Open initial image
  await pathInput.fill('/Volumes/Photos/session1.jpg');
  await pathInput.press('Enter');
  await page.waitForTimeout(300);
  await blurAll();

  // -------------------------------------------------------------------------
  // 1. Two slider commits, then Cmd-Z: the first's value is on screen, a render
  //    with it was requested, and a save with it was sent. Cmd-Shift-Z brings the second back.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 1: Two slider commits, then Cmd-Z and Cmd-Shift-Z...');
  await page.evaluate(() => {
    window.__STUB__.renders = [];
    window.__STUB__.saveCalls = [];
  });

  // Slider 1: Exposure to 1.0
  await setSliderValue('exposure', 1.0);
  await page.waitForTimeout(400);

  // Slider 2: Contrast to 20
  await setSliderValue('contrast', 20);
  await page.waitForTimeout(400);

  // Press Cmd-Z
  await page.evaluate(() => {
    window.__STUB__.renders = [];
    window.__STUB__.saveCalls = [];
  });
  await pressUndo();
  await page.waitForTimeout(450);

  // Value on screen should be exposure=1.0, contrast=0
  const expVal1 = await page.locator('input[data-testid="exposure-number"]').inputValue();
  const contrastVal1 = await page.locator('input[data-testid="contrast-number"]').inputValue();
  if (expVal1 !== '1.0' && expVal1 !== '1') {
    failures.push(`Assertion 1: Expected exposure=1.0 after undo, got ${expVal1}`);
  }
  if (contrastVal1 !== '0') {
    failures.push(`Assertion 1: Expected contrast=0 after undo, got ${contrastVal1}`);
  }

  // Check render requested with exposure=1.0, contrast=0
  const renders1 = await page.evaluate(() => window.__STUB__.renders);
  const settleRender1 = renders1.find((r) => r.stage === 'Settle' && r.recipe.exposure === 1.0 && r.recipe.contrast === 0);
  if (!settleRender1) {
    failures.push('Assertion 1: Settle render with exposure=1.0 and contrast=0 was not requested after undo');
  }

  // Check save sent with exposure=1.0, contrast=0
  const saveCalls1 = await page.evaluate(() => window.__STUB__.saveCalls);
  const save1 = saveCalls1.find((s) => s.recipe.exposure === 1.0 && s.recipe.contrast === 0);
  if (!save1) {
    failures.push('Assertion 1: Save with exposure=1.0 and contrast=0 was not sent after undo');
  }

  // Cmd-Shift-Z brings second back
  await page.evaluate(() => {
    window.__STUB__.renders = [];
    window.__STUB__.saveCalls = [];
  });
  await pressRedo();
  await page.waitForTimeout(450);

  const expValRedo = await page.locator('input[data-testid="exposure-number"]').inputValue();
  const contrastValRedo = await page.locator('input[data-testid="contrast-number"]').inputValue();
  if ((expValRedo !== '1.0' && expValRedo !== '1') || contrastValRedo !== '20') {
    failures.push(`Assertion 1: Expected exposure=1.0, contrast=20 after redo, got exp=${expValRedo}, contrast=${contrastValRedo}`);
  }

  // -------------------------------------------------------------------------
  // 2. Twenty input events and one release make exactly one step.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 2: Twenty input events and one release make exactly one step...');
  // Open fresh photograph
  await pathInput.fill('/Volumes/Photos/step2.jpg');
  await pathInput.press('Enter');
  await page.waitForTimeout(300);
  await blurAll();

  // Send 20 input events on exposure range slider, then one change event
  await page.evaluate(() => {
    const slider = document.querySelector('input[data-testid="exposure-slider"]');
    if (!slider) throw new Error('Exposure slider not found');
    for (let i = 1; i <= 20; i++) {
      slider.value = String(i * 0.1);
      slider.dispatchEvent(new Event('input', { bubbles: true }));
    }
    slider.dispatchEvent(new Event('change', { bubbles: true }));
  });
  await page.waitForTimeout(400);

  // Open history section if closed
  const historySecHeader = page.locator('[data-testid="section-history"] .collapsible-section__header');
  const historyPanel = page.locator('[data-testid="history-panel"]');
  if (!(await historyPanel.isVisible())) {
    await historySecHeader.click();
    await page.waitForTimeout(100);
  }

  const historyItems2 = await page.locator('.history-item').count();
  if (historyItems2 !== 1) {
    failures.push(`Assertion 2: Expected exactly 1 history step after 20 inputs + 1 release, got ${historyItems2}`);
  }

  // Undoing once reverts it back to 0
  await pressUndo();
  await page.waitForTimeout(350);
  const expAfterUndo2 = await page.locator('input[data-testid="exposure-number"]').inputValue();
  if (expAfterUndo2 !== '0.0' && expAfterUndo2 !== '0') {
    failures.push(`Assertion 2: Undoing single step did not revert exposure to 0, got ${expAfterUndo2}`);
  }

  // -------------------------------------------------------------------------
  // 3. A new change after an undo removes the redo.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 3: A new change after an undo removes the redo...');
  // Make a change: Whites = 10
  await setSliderValue('whites', 10);
  await page.waitForTimeout(400);
  // Undo: whites goes back to 0
  await pressUndo();
  await page.waitForTimeout(350);
  // Redo is currently available. Now make a NEW change: Blacks = -15
  await setSliderValue('blacks', -15);
  await page.waitForTimeout(400);

  // Try to Redo: should do nothing because redo was discarded
  await pressRedo();
  await page.waitForTimeout(350);
  const blacksVal3 = await page.locator('input[data-testid="blacks-number"]').inputValue();
  if (blacksVal3 !== '-15') {
    failures.push(`Assertion 3: Redo was not discarded after new change, blacks is ${blacksVal3}`);
  }

  // -------------------------------------------------------------------------
  // 4. Cmd-Z with nothing to undo does nothing and sends nothing.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 4: Cmd-Z with nothing to undo does nothing and sends nothing...');
  // Open fresh photograph
  await pathInput.fill('/Volumes/Photos/step4.jpg');
  await pathInput.press('Enter');
  await page.waitForTimeout(300);
  await blurAll();

  await page.evaluate(() => {
    window.__STUB__.renders = [];
    window.__STUB__.saveCalls = [];
  });
  await pressUndo();
  await page.waitForTimeout(400);

  const st4 = await page.evaluate(() => ({
    renders: window.__STUB__.renders.length,
    saves: window.__STUB__.saveCalls.length,
  }));
  if (st4.renders > 0 || st4.saves > 0) {
    failures.push(`Assertion 4: Cmd-Z with nothing to undo triggered renders (${st4.renders}) or saves (${st4.saves})`);
  }

  // -------------------------------------------------------------------------
  // 5. The list shows the labels, newest first, and the current one is marked;
  //    clicking an older one restores it.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 5: The list shows labels newest first, current marked, clicking older restores it...');
  await pathInput.fill('/Volumes/Photos/step5.jpg');
  await pathInput.press('Enter');
  await page.waitForTimeout(300);
  await blurAll();

  // Commit Exposure, then Contrast
  await setSliderValue('exposure', 0.5);
  await page.waitForTimeout(400);
  await setSliderValue('contrast', 15);
  await page.waitForTimeout(400);

  if (!(await historyPanel.isVisible())) {
    await historySecHeader.click();
    await page.waitForTimeout(100);
  }

  const items5 = page.locator('.history-item');
  const count5 = await items5.count();
  if (count5 !== 2) {
    failures.push(`Assertion 5: Expected 2 history items, got ${count5}`);
  } else {
    const text0 = (await items5.nth(0).innerText()).trim();
    const text1 = (await items5.nth(1).innerText()).trim();
    if (!text0.includes('Contrast')) {
      failures.push(`Assertion 5: First item (newest) should be Contrast, got "${text0}"`);
    }
    if (!text1.includes('Exposure')) {
      failures.push(`Assertion 5: Second item should be Exposure, got "${text1}"`);
    }

    // Current one is marked
    const is0Current = await items5.nth(0).getAttribute('data-current');
    if (is0Current !== 'true') {
      failures.push('Assertion 5: Newest item should be marked current');
    }

    // Click older one (Exposure)
    await items5.nth(1).click();
    await page.waitForTimeout(400);

    const contrastAfterJump = await page.locator('input[data-testid="contrast-number"]').inputValue();
    const expAfterJump = await page.locator('input[data-testid="exposure-number"]').inputValue();
    if (contrastAfterJump !== '0') {
      failures.push(`Assertion 5: Clicking older item did not restore contrast to 0, got ${contrastAfterJump}`);
    }
    if (expAfterJump !== '0.5') {
      failures.push(`Assertion 5: Exposure should be 0.5, got ${expAfterJump}`);
    }
    const is1Current = await items5.nth(1).getAttribute('data-current');
    if (is1Current !== 'true') {
      failures.push('Assertion 5: Older item should now be marked current');
    }
  }

  // -------------------------------------------------------------------------
  // 6. A preset, a crop applied, a mask added, Auto, the eyedropper and black & white
  //    are each one step, each with its proper label, and each undone.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 6: Preset, crop, mask, Auto, eyedropper, B&W are each one step with proper label and undone...');

  // Helper to test an action produces a step with expected label, and Cmd-Z undoes it
  const testActionUndo = async (actionDesc, doAction, expectedLabelSubstring, verifyUndone) => {
    await pathInput.fill(`/Volumes/Photos/step6_${actionDesc}.jpg`);
    await pathInput.press('Enter');
    await page.waitForTimeout(300);
    await blurAll();

    if (!(await historyPanel.isVisible())) {
      await historySecHeader.click();
      await page.waitForTimeout(100);
    }

    await doAction();
    await page.waitForTimeout(450);

    const items = page.locator('.history-item');
    const count = await items.count();
    if (count !== 1) {
      failures.push(`Assertion 6 (${actionDesc}): Expected exactly 1 step, got ${count}`);
    } else {
      const label = (await items.first().innerText()).trim();
      if (!label.toLowerCase().includes(expectedLabelSubstring.toLowerCase())) {
        failures.push(`Assertion 6 (${actionDesc}): Expected label to include "${expectedLabelSubstring}", got "${label}"`);
      }
    }

    await pressUndo();
    await page.waitForTimeout(450);
    await verifyUndone();
  };

  // Save preset first on session image so preset library has TestPreset
  await setSliderValue('exposure', 0.8);
  await page.waitForTimeout(400);
  await page.locator('button[data-testid="preset-save-btn"]').click();
  await page.waitForTimeout(100);
  await page.locator('input[data-testid="preset-name-input"]').fill('TestPreset');
  await page.locator('button[data-testid="preset-submit-btn"]').click();
  await page.waitForTimeout(300);

  // a. Preset
  await testActionUndo(
    'preset',
    async () => {
      await page.locator('select[data-testid="preset-select"]').selectOption('TestPreset');
    },
    'TestPreset',
    async () => {
      const exp = await page.locator('input[data-testid="exposure-number"]').inputValue();
      if (exp !== '0.0' && exp !== '0') {
        failures.push(`Assertion 6 (preset): Undo did not revert exposure, got ${exp}`);
      }
    }
  );

  // b. Crop applied
  await testActionUndo(
    'crop',
    async () => {
      const geomHeader = page.locator('[data-testid="section-geometry"] .collapsible-section__header');
      if (!(await page.locator('button[data-testid="crop-mode-toggle-btn"]').isVisible())) {
        await geomHeader.click();
        await page.waitForTimeout(100);
      }
      const cropBtn = page.locator('button[data-testid="crop-mode-toggle-btn"]');
      await cropBtn.click();
      await page.waitForTimeout(150);
      await cropBtn.click();
    },
    'Crop',
    async () => {
      const recipe = await page.evaluate(() => window.__STUB__.lastSavedRecipe);
      if (recipe && recipe.geometry && recipe.geometry.crop) {
        failures.push('Assertion 6 (crop): Undo did not revert crop');
      }
    }
  );

  // c. Mask added
  await testActionUndo(
    'mask',
    async () => {
      const masksPanel = page.locator('[data-testid="masks-panel"]');
      if (!(await masksPanel.isVisible())) {
        await page.locator('[data-testid="section-masks"] .collapsible-section__header').click();
        await page.waitForTimeout(100);
      }
      const addRadialBtn = page.locator('button[data-testid="add-radial-mask"]');
      await addRadialBtn.click();
    },
    'mask',
    async () => {
      const count = await page.locator('.mask-row').count();
      if (count !== 0) {
        failures.push(`Assertion 6 (mask): Undo did not remove mask, mask count is ${count}`);
      }
    }
  );

  // d. Auto Tone
  await testActionUndo(
    'auto',
    async () => {
      await page.evaluate(() => {
        window.__STUB__.autoToneResult = { exposure: 0.75, whites: 12, blacks: -10 };
      });
      const autoBtn = page.locator('button[data-testid="auto-tone-btn"]');
      await autoBtn.click();
    },
    'Auto',
    async () => {
      const exp = await page.locator('input[data-testid="exposure-number"]').inputValue();
      if (exp !== '0.0' && exp !== '0') {
        failures.push(`Assertion 6 (auto): Undo did not revert exposure, got ${exp}`);
      }
    }
  );

  // e. Eyedropper
  await testActionUndo(
    'eyedropper',
    async () => {
      await page.evaluate(() => {
        window.__STUB__.whiteBalanceResult = { temperature: 20, tint: -10, clamped: false };
      });
      await blurAll();
      await page.keyboard.press('w');
      await page.waitForTimeout(100);
      const canvas = page.locator('canvas[data-testid="edit-canvas"]');
      const box = await canvas.boundingBox();
      if (box) {
        await page.mouse.click(box.x + box.width * 0.5, box.y + box.height * 0.5);
      }
    },
    'White balance',
    async () => {
      const temp = await page.locator('input[data-testid="temperature-number"]').inputValue();
      if (temp !== '0') {
        failures.push(`Assertion 6 (eyedropper): Undo did not revert temperature to 0, got ${temp}`);
      }
    }
  );

  // f. Black & white
  await testActionUndo(
    'black_and_white',
    async () => {
      await blurAll();
      await page.keyboard.press('v');
    },
    'Black & white',
    async () => {
      const isBw = await page.evaluate(() => {
        return !!window.__STUB__.lastSavedRecipe?.black_and_white;
      });
      if (isBw) {
        failures.push('Assertion 6 (black_and_white): Undo did not revert black & white');
      }
    }
  );

  // -------------------------------------------------------------------------
  // 7. Undoing the creation of the selected mask deselects it.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 7: Undoing the creation of selected mask deselects it...');
  await pathInput.fill('/Volumes/Photos/step7.jpg');
  await pathInput.press('Enter');
  await page.waitForTimeout(300);
  await blurAll();

  // Add mask
  const masksPanel7 = page.locator('[data-testid="masks-panel"]');
  if (!(await masksPanel7.isVisible())) {
    await page.locator('[data-testid="section-masks"] .collapsible-section__header').click();
    await page.waitForTimeout(100);
  }
  await page.locator('button[data-testid="add-radial-mask"]').click();
  await page.waitForTimeout(300);

  // Verify mask is selected (global adjustments put away notice visible)
  const putAwayVisibleBefore = await page.locator('[data-testid="global-put-away"]').isVisible();
  if (!putAwayVisibleBefore) {
    failures.push('Assertion 7: Global put-away notice should be visible when mask is selected');
  }

  // Undo mask creation
  await pressUndo();
  await page.waitForTimeout(400);

  const putAwayVisibleAfter = await page.locator('[data-testid="global-put-away"]').isVisible();
  if (putAwayVisibleAfter) {
    failures.push('Assertion 7: Mask should be deselected after undoing its creation');
  }

  // -------------------------------------------------------------------------
  // 8. With a text field focused, Cmd-Z is not intercepted.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 8: With a text field focused, Cmd-Z is not intercepted...');
  await pathInput.fill('/Volumes/Photos/step8.jpg');
  await pathInput.press('Enter');
  await page.waitForTimeout(300);

  // Commit an edit first
  await setSliderValue('exposure', 1.0);
  await page.waitForTimeout(400);

  // Focus snapshot name input and type
  const snapSecHeader = page.locator('[data-testid="section-snapshots"] .collapsible-section__header');
  const snapPanel = page.locator('[data-testid="snapshots-panel"]');
  if (!(await snapPanel.isVisible())) {
    await snapSecHeader.click();
    await page.waitForTimeout(100);
  }
  const snapInput = page.locator('input[data-testid="snapshot-name-input"]');
  await snapInput.click();
  await snapInput.fill('SnapshotOne');
  // Press Cmd-Z while focused
  await page.keyboard.press('Meta+z');
  await page.waitForTimeout(200);

  // Photograph's exposure should still be 1.0 (not undone!)
  const expVal8 = await page.locator('input[data-testid="exposure-number"]').inputValue();
  if (expVal8 !== '1.0' && expVal8 !== '1') {
    failures.push(`Assertion 8: Cmd-Z was intercepted while text input was focused (exposure became ${expVal8})`);
  }

  // -------------------------------------------------------------------------
  // 9. In crop mode, Cmd-Z does nothing.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 9: In crop mode, Cmd-Z does nothing...');
  await pathInput.fill('/Volumes/Photos/step9.jpg');
  await pathInput.press('Enter');
  await page.waitForTimeout(300);

  // Commit an edit first
  await setSliderValue('exposure', 0.8);
  await page.waitForTimeout(400);

  // Enter crop mode
  const geomHeader9 = page.locator('[data-testid="section-geometry"] .collapsible-section__header');
  if (!(await page.locator('button[data-testid="crop-mode-toggle-btn"]').isVisible())) {
    await geomHeader9.click();
    await page.waitForTimeout(100);
  }
  const cropToggleBtn = page.locator('button[data-testid="crop-mode-toggle-btn"]');
  await cropToggleBtn.click();
  await page.waitForTimeout(200);

  // Press Cmd-Z
  await page.keyboard.press('Meta+z');
  await page.waitForTimeout(300);

  // Crop overlay should still be active, exposure still 0.8
  const inCropMode = await page.locator('.crop-overlay').isVisible();
  if (!inCropMode) {
    failures.push('Assertion 9: Cmd-Z exited or broke crop mode');
  }
  const expVal9 = await page.locator('input[data-testid="exposure-number"]').inputValue();
  if (expVal9 !== '0.8') {
    failures.push(`Assertion 9: Exposure should still be 0.8 in crop mode, got ${expVal9}`);
  }
  await cropToggleBtn.click();
  await page.waitForTimeout(200);

  // -------------------------------------------------------------------------
  // 10. Opening another photograph empties the list.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 10: Opening another photograph empties the list...');
  // Commit edits on photo A
  await pathInput.fill('/Volumes/Photos/photoA.jpg');
  await pathInput.press('Enter');
  await page.waitForTimeout(300);
  await setSliderValue('exposure', 1.2);
  await page.waitForTimeout(400);

  if (!(await historyPanel.isVisible())) {
    await historySecHeader.click();
    await page.waitForTimeout(100);
  }
  const itemsBefore10 = await page.locator('.history-item').count();
  if (itemsBefore10 === 0) {
    failures.push('Assertion 10: Precondition failed: history was empty before opening photo B');
  }

  // Open photo B
  await pathInput.fill('/Volumes/Photos/photoB.jpg');
  await pathInput.press('Enter');
  await page.waitForTimeout(300);

  const itemsAfter10 = await page.locator('.history-item').count();
  const emptyVisible10 = await page.locator('[data-testid="history-empty"]').isVisible();
  if (itemsAfter10 !== 0 || !emptyVisible10) {
    failures.push(`Assertion 10: Opening another photograph did not empty the list (items: ${itemsAfter10}, empty visible: ${emptyVisible10})`);
  }

  // -------------------------------------------------------------------------
  // 11. A snapshot appears in the save payload; restoring it is one step;
  //     undoing puts back what was there and leaves the snapshot in the list;
  //     deleting removes it from the next save.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 11: Snapshot save payload, restore as 1 step, undo restores recipe & keeps snapshot, delete removes from save...');
  await pathInput.fill('/Volumes/Photos/step11.jpg');
  await pathInput.press('Enter');
  await page.waitForTimeout(300);

  if (!(await snapPanel.isVisible())) {
    await snapSecHeader.click();
    await page.waitForTimeout(100);
  }

  // Save snapshot "StateA" (identity)
  await snapInput.fill('StateA');
  await page.locator('button[data-testid="snapshot-save-btn"]').click();
  await page.waitForTimeout(450);

  // Check snapshot in save payload
  const saved11a = await page.evaluate(() => window.__STUB__.lastSavedRecipe);
  if (!saved11a?.snapshots?.some((s) => s.name === 'StateA')) {
    failures.push(`Assertion 11: Snapshot StateA did not appear in save payload (${JSON.stringify(saved11a?.snapshots)})`);
  }

  // Make an edit: Exposure = 1.5
  await setSliderValue('exposure', 1.5);
  await page.waitForTimeout(400);

  // Restore snapshot StateA
  const restoreBtn = page.locator('button[data-testid="snapshot-restore-StateA"]');
  await restoreBtn.click();
  await page.waitForTimeout(450);

  // Restored: exposure should be 0.0
  const expAfterRestore11 = await page.locator('input[data-testid="exposure-number"]').inputValue();
  if (expAfterRestore11 !== '0.0' && expAfterRestore11 !== '0') {
    failures.push(`Assertion 11: Restoring StateA did not revert exposure to 0, got ${expAfterRestore11}`);
  }

  // Undo restore: puts back exposure = 1.5
  await pressUndo();
  await page.waitForTimeout(450);
  const expAfterUndo11 = await page.locator('input[data-testid="exposure-number"]').inputValue();
  if (expAfterUndo11 !== '1.5') {
    failures.push(`Assertion 11: Undoing restore did not put back exposure=1.5, got ${expAfterUndo11}`);
  }

  // Snapshot StateA should still be in the list
  const stateAStillInList = await page.locator('[data-testid="snapshot-item-StateA"]').isVisible();
  if (!stateAStillInList) {
    failures.push('Assertion 11: Undoing restore did not leave snapshot StateA in the list');
  }

  // Delete snapshot StateA
  const deleteBtn = page.locator('button[data-testid="snapshot-delete-StateA"]');
  await deleteBtn.click();
  await page.waitForTimeout(450);

  const saved11b = await page.evaluate(() => window.__STUB__.lastSavedRecipe);
  if (saved11b?.snapshots?.some((s) => s.name === 'StateA')) {
    failures.push('Assertion 11: Deleting StateA did not remove it from the next save payload');
  }

  // -------------------------------------------------------------------------
  // 12. An empty and a duplicate name are refused with a visible message.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 12: Empty and duplicate names refused with visible message...');
  await snapInput.fill('');
  await page.locator('button[data-testid="snapshot-save-btn"]').click();
  await page.waitForTimeout(100);
  const emptyMsg = await page.locator('[data-testid="snapshot-message"]').innerText();
  if (!emptyMsg.toLowerCase().includes('empty')) {
    failures.push(`Assertion 12: Expected empty message refusal, got "${emptyMsg}"`);
  }

  // Save a valid one
  await snapInput.fill('UniqueName');
  await page.locator('button[data-testid="snapshot-save-btn"]').click();
  await page.waitForTimeout(300);

  // Try to save duplicate
  await snapInput.fill('UniqueName');
  await page.locator('button[data-testid="snapshot-save-btn"]').click();
  await page.waitForTimeout(100);
  const dupMsg = await page.locator('[data-testid="snapshot-message"]').innerText();
  if (!dupMsg.toLowerCase().includes('already exists') && !dupMsg.toLowerCase().includes('duplicate')) {
    failures.push(`Assertion 12: Expected duplicate message refusal, got "${dupMsg}"`);
  }

  // -------------------------------------------------------------------------
  // 13. Pasting settings from a photograph with snapshots brings none.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 13: Pasting settings from photograph with snapshots brings none...');
  // Photo A has snapshot "UniqueName". Copy its settings.
  await page.locator('button[data-testid="copy-settings-btn"]').click();
  await page.waitForTimeout(200);

  // Open fresh Photo C
  await pathInput.fill('/Volumes/Photos/photoC.jpg');
  await pathInput.press('Enter');
  await page.waitForTimeout(300);

  // Paste settings
  await page.locator('button[data-testid="paste-settings-btn"]').click();
  await page.waitForTimeout(450);

  // Saved recipe for photo C should have no snapshots
  const saved13 = await page.evaluate(() => window.__STUB__.lastSavedRecipe);
  if (saved13?.snapshots && saved13.snapshots.length > 0) {
    failures.push(`Assertion 13: Pasting settings brought snapshots: ${JSON.stringify(saved13.snapshots)}`);
  }

  // -------------------------------------------------------------------------
  // 14. On a read-only photograph, undo works and no save is sent.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 14: On a read-only photograph, undo works and no save is sent...');
  await page.evaluate(() => {
    window.__STUB__.readOnly = true;
  });
  await pathInput.fill('/Volumes/Photos/readonly.jpg');
  await pathInput.press('Enter');
  await page.waitForTimeout(300);

  await page.evaluate(() => {
    window.__STUB__.saveCalls = [];
  });

  // Make change
  await setSliderValue('exposure', 0.9);
  await page.waitForTimeout(400);

  // Verify save was not sent
  const savesAfterChange14 = await page.evaluate(() => window.__STUB__.saveCalls.length);
  if (savesAfterChange14 !== 0) {
    failures.push(`Assertion 14: Save was sent on read-only photograph after slider change (${savesAfterChange14})`);
  }

  // Undo
  await pressUndo();
  await page.waitForTimeout(400);

  const expVal14 = await page.locator('input[data-testid="exposure-number"]').inputValue();
  if (expVal14 !== '0.0' && expVal14 !== '0') {
    failures.push(`Assertion 14: Undo did not work on read-only photograph, exposure is ${expVal14}`);
  }
  const savesAfterUndo14 = await page.evaluate(() => window.__STUB__.saveCalls.length);
  if (savesAfterUndo14 !== 0) {
    failures.push(`Assertion 14: Save was sent on read-only photograph after undo (${savesAfterUndo14})`);
  }

  await page.evaluate(() => {
    window.__STUB__.readOnly = false;
  });

  // -------------------------------------------------------------------------
  // 15. EditHistory alone: 150 commits keep 100; a set of large recipes stays under maxBytes, oldest gone, the current kept.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 15: EditHistory alone (maxEntries limit and maxBytes limit)...');
  const h15Results = await page.evaluate(() => {
    const EditHistory = window.__EditHistory__;
    const errs = [];
    const h1 = new EditHistory(100, 64 * 1024 * 1024);
    const baseRecipe = { version: 2, exposure: 0 };
    h1.reset(baseRecipe);
    for (let i = 1; i <= 150; i++) {
      h1.commit({ version: 2, exposure: i }, `Step ${i}`);
    }
    if (h1.entries.length !== 100) {
      errs.push(`150 commits should keep 100, got ${h1.entries.length}`);
    }
    if (h1.cursor !== 99) {
      errs.push(`Cursor should be at 99, got ${h1.cursor}`);
    }
    if (h1.entries[h1.entries.length - 1].label !== 'Step 150') {
      errs.push(`Last entry should be Step 150, got ${h1.entries[h1.entries.length - 1].label}`);
    }
    if (h1.entries[0].label !== 'Step 51') {
      errs.push(`Oldest kept entry should be Step 51, got ${h1.entries[0].label}`);
    }

    // Byte limit
    const maxBytes = 10000;
    const h2 = new EditHistory(100, maxBytes);
    h2.reset(baseRecipe);
    const largeBlob = 'x'.repeat(3000);
    for (let i = 1; i <= 10; i++) {
      h2.commit({ version: 2, exposure: i, dummy: largeBlob }, `Large ${i}`);
    }
    if (h2.entries.length >= 10) {
      errs.push(`Byte limit did not evict entries, length is ${h2.entries.length}`);
    }
    if (h2.entries[h2.entries.length - 1].label !== 'Large 10') {
      errs.push(`Current entry should be kept, got ${h2.entries[h2.entries.length - 1].label}`);
    }
    return errs;
  });
  for (const err of h15Results) {
    failures.push(`Assertion 15: ${err}`);
  }

  // -------------------------------------------------------------------------
  // 16. Every new control is at least 40 px. Screenshot to layout-proof/.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 16: Every new control is at least 40 px; screenshot to layout-proof/...');
  // Load clean image and open both History and Snapshots
  await pathInput.fill('/Volumes/Photos/final_layout.jpg');
  await pathInput.press('Enter');
  await page.waitForTimeout(300);

  // Add 2 history steps
  await setSliderValue('exposure', 0.5);
  await page.waitForTimeout(400);
  await setSliderValue('contrast', 10);
  await page.waitForTimeout(400);

  // Add a snapshot
  if (!(await snapPanel.isVisible())) {
    await snapSecHeader.click();
    await page.waitForTimeout(100);
  }
  await snapInput.fill('ProofSnapshot');
  await page.locator('button[data-testid="snapshot-save-btn"]').click();
  await page.waitForTimeout(400);

  if (!(await historyPanel.isVisible())) {
    await historySecHeader.click();
    await page.waitForTimeout(100);
  }

  const controlsToCheck = [
    { name: 'History section header', selector: '[data-testid="section-history"] .collapsible-section__header' },
    { name: 'History item', selector: '.history-item' },
    { name: 'Snapshots section header', selector: '[data-testid="section-snapshots"] .collapsible-section__header' },
    { name: 'Snapshot name input', selector: 'input[data-testid="snapshot-name-input"]' },
    { name: 'Snapshot save button', selector: 'button[data-testid="snapshot-save-btn"]' },
    { name: 'Snapshot restore button', selector: 'button[data-testid="snapshot-restore-ProofSnapshot"]' },
    { name: 'Snapshot delete button', selector: 'button[data-testid="snapshot-delete-ProofSnapshot"]' },
  ];

  for (const ctrl of controlsToCheck) {
    const el = page.locator(ctrl.selector).first();
    const box = await el.boundingBox();
    if (!box) {
      failures.push(`Assertion 16: ${ctrl.name} not found or not visible`);
      continue;
    }
    if (box.height < 39.5) {
      failures.push(`Assertion 16: ${ctrl.name} height (${box.height.toFixed(1)}px) is under 40px`);
    }
    if (box.width < 39.5) {
      failures.push(`Assertion 16: ${ctrl.name} width (${box.width.toFixed(1)}px) is under 40px`);
    }
  }

  const screenshotPath = join(OUT, 'history.png');
  await page.screenshot({ path: screenshotPath });
  console.log(`Saved proof screenshot to ${screenshotPath}`);

} catch (err) {
  failures.push(`Execution error: ${err instanceof Error ? err.stack : String(err)}`);
} finally {
  await browser.close();
  await server.close();
}

console.log(`\n========================================`);
console.log(`Assertions run: ${assertionsRun} / 16`);
if (failures.length === 0) {
  console.log(`All ${assertionsRun} assertions PASSED cleanly.`);
  process.exit(0);
} else {
  console.error(`FAILED with ${failures.length} issues:`);
  for (const f of failures) {
    console.error(`- ${f}`);
  }
  process.exit(1);
}
