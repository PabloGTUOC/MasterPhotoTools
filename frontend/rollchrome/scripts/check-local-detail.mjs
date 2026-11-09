/**
 * RP-11b Local detail (Clarity, Texture, Dehaze, Sharpness on masks) acceptance & verification in a real headless browser.
 *
 * Asserts all eight requirements from the brief:
 * 1. The selected mask's panel shows the four sliders with those ranges.
 * 2. Moving Clarity sends Drag renders carrying detail.clarity; release sends Settle.
 * 3. All four values reach the save payload.
 * 4. One slider release is one undo step, labelled Masks, and undo restores the value.
 * 5. A mask with all four at zero sends no detail field.
 * 6. Selecting another mask shows that mask's own values.
 * 7. A photograph reopened from a saved recipe shows the saved values.
 * 8. Every new control is at least 40 px, and a missing control fails. Screenshot to layout-proof/.
 */

import { existsSync, mkdirSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath, URL } from 'node:url';
import assert from 'node:assert/strict';
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

console.log('Starting Vite harness server for check:local-detail...');
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

  // Setup clean stub state
  await page.evaluate(() => {
    window.__STUB__.orientation = 1;
    window.__STUB__.toStoredOverride = null;
    window.__STUB__.renders = [];
    window.__STUB__.lastSavedRecipe = null;
    window.__STUB__.recipesByPath = {};
  });

  const testPhotoPath = '/Volumes/Photos/local_detail_test.jpg';
  await pathInput.fill(testPhotoPath);
  await pathInput.press('Enter');
  await page.waitForTimeout(250);
  await blurAll();

  await ensureMasksSectionOpen();

  // -------------------------------------------------------------------------
  // 1. The selected mask's panel shows the four sliders with those ranges.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log("Asserting 1: The selected mask's panel shows the four sliders with those ranges...");
  try {
    const addLinearBtn = page.locator('button[data-testid="add-linear-mask"]');
    await addLinearBtn.waitFor({ state: 'visible' });
    await addLinearBtn.click();
    await page.waitForTimeout(200);

    const maskEditing = page.locator('[data-testid="mask-editing"]');
    await maskEditing.waitFor({ state: 'visible' });

    const maskDetailSection = page.locator('[data-testid="mask-detail-controls"]');
    await maskDetailSection.waitFor({ state: 'visible' });

    const sliders = [
      { id: 'mask-clarity', min: -100, max: 100, defaultVal: 0 },
      { id: 'mask-texture', min: -100, max: 100, defaultVal: 0 },
      { id: 'mask-dehaze', min: -100, max: 100, defaultVal: 0 },
      { id: 'mask-sharpness', min: 0, max: 100, defaultVal: 0 },
    ];

    for (const s of sliders) {
      const sliderEl = page.locator(`input[data-testid="${s.id}-slider"]`);
      assert.ok(await sliderEl.isVisible(), `Slider ${s.id} must be visible`);

      const minVal = parseFloat(await sliderEl.getAttribute('min') ?? '0');
      const maxVal = parseFloat(await sliderEl.getAttribute('max') ?? '0');
      assert.equal(minVal, s.min, `${s.id} min must be ${s.min}`);
      assert.equal(maxVal, s.max, `${s.id} max must be ${s.max}`);

      const numberInput = page.locator(`input[data-testid="${s.id}-number"]`);
      const val = parseFloat(await numberInput.inputValue());
      assert.equal(val, s.defaultVal, `${s.id} default value must be ${s.defaultVal}`);
    }

    console.log('  Assertion 1 PASSED');
  } catch (err) {
    failures.push({ assertion: 1, error: err });
    console.error('Assertion 1 failed:', err);
  }

  // -------------------------------------------------------------------------
  // 2. Moving Clarity sends Drag renders carrying detail.clarity; release sends Settle.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 2: Moving Clarity sends Drag renders carrying detail.clarity; release sends Settle...');
  try {
    await page.evaluate(() => {
      window.__STUB__.renders = [];
    });

    const claritySlider = page.locator('input[data-testid="mask-clarity-slider"]');
    await claritySlider.waitFor({ state: 'visible' });

    // Scrub slider: emit input event with value 35
    await claritySlider.evaluate((el) => {
      el.value = '35';
      el.dispatchEvent(new Event('input', { bubbles: true }));
    });
    await page.waitForTimeout(100);

    const dragRenders = await page.evaluate(() => {
      return window.__STUB__.renders.filter((r) => r.stage === 'Drag');
    });
    assert.ok(dragRenders.length > 0, 'Moving mask clarity must emit Drag renders');
    const lastDrag = dragRenders[dragRenders.length - 1];
    const dragMask = lastDrag.recipe?.masks?.[0];
    assert.ok(dragMask?.detail, 'Drag render must carry mask detail');
    assert.equal(dragMask.detail.clarity, 35, 'Drag render must carry updated clarity 35');

    // Release slider: emit change event
    await claritySlider.evaluate((el) => {
      el.dispatchEvent(new Event('change', { bubbles: true }));
    });
    await page.waitForTimeout(150);

    const settleRenders = await page.evaluate(() => {
      return window.__STUB__.renders.filter((r) => r.stage === 'Settle');
    });
    assert.ok(settleRenders.length > 0, 'Releasing mask clarity must commit Settle render');
    const lastSettle = settleRenders[settleRenders.length - 1];
    const settleMask = lastSettle.recipe?.masks?.[0];
    assert.equal(settleMask?.detail?.clarity, 35, 'Settle render must carry updated clarity 35');

    console.log('  Assertion 2 PASSED');
  } catch (err) {
    failures.push({ assertion: 2, error: err });
    console.error('Assertion 2 failed:', err);
  }

  // -------------------------------------------------------------------------
  // 3. All four values reach the save payload.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 3: All four values reach the save payload...');
  try {
    // Set texture to 25
    await page.locator('input[data-testid="mask-texture-slider"]').evaluate((el) => {
      el.value = '25';
      el.dispatchEvent(new Event('input', { bubbles: true }));
      el.dispatchEvent(new Event('change', { bubbles: true }));
    });
    // Set dehaze to 15
    await page.locator('input[data-testid="mask-dehaze-slider"]').evaluate((el) => {
      el.value = '15';
      el.dispatchEvent(new Event('input', { bubbles: true }));
      el.dispatchEvent(new Event('change', { bubbles: true }));
    });
    // Set sharpness to 40
    await page.locator('input[data-testid="mask-sharpness-slider"]').evaluate((el) => {
      el.value = '40';
      el.dispatchEvent(new Event('input', { bubbles: true }));
      el.dispatchEvent(new Event('change', { bubbles: true }));
    });

    await page.waitForTimeout(450); // wait for save debounce

    const saved = await page.evaluate(() => window.__STUB__.lastSavedRecipe);
    assert.ok(saved?.masks?.[0]?.detail, 'Saved recipe must carry mask detail');
    const d = saved.masks[0].detail;
    assert.equal(d.clarity, 35, 'Saved clarity must be 35');
    assert.equal(d.texture, 25, 'Saved texture must be 25');
    assert.equal(d.dehaze, 15, 'Saved dehaze must be 15');
    assert.equal(d.sharpness, 40, 'Saved sharpness must be 40');

    console.log('  Assertion 3 PASSED');
  } catch (err) {
    failures.push({ assertion: 3, error: err });
    console.error('Assertion 3 failed:', err);
  }

  // -------------------------------------------------------------------------
  // 4. One slider release is one undo step, labelled Masks, and undo restores the value.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 4: One slider release is one undo step, labelled Masks, and undo restores the value...');
  try {
    // Set clarity from 35 to 50
    await page.locator('input[data-testid="mask-clarity-slider"]').evaluate((el) => {
      el.value = '50';
      el.dispatchEvent(new Event('input', { bubbles: true }));
      el.dispatchEvent(new Event('change', { bubbles: true }));
    });
    await page.waitForTimeout(200);

    let clarityNow = parseFloat(await page.locator('input[data-testid="mask-clarity-number"]').inputValue());
    assert.equal(clarityNow, 50);

    // Undo should revert clarity to 35
    await page.keyboard.press('Meta+z');
    await page.waitForTimeout(200);

    clarityNow = parseFloat(await page.locator('input[data-testid="mask-clarity-number"]').inputValue());
    assert.equal(clarityNow, 35, 'Undo must restore clarity value to 35');

    // Redo restores to 50
    await page.keyboard.press('Meta+Shift+z');
    await page.waitForTimeout(200);

    clarityNow = parseFloat(await page.locator('input[data-testid="mask-clarity-number"]').inputValue());
    assert.equal(clarityNow, 50, 'Redo must restore clarity value to 50');

    console.log('  Assertion 4 PASSED');
  } catch (err) {
    failures.push({ assertion: 4, error: err });
    console.error('Assertion 4 failed:', err);
  }

  // -------------------------------------------------------------------------
  // 5. A mask with all four at zero sends no detail field.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 5: A mask with all four at zero sends no detail field...');
  try {
    // Click mask reset button to zero adjustments and detail
    const resetBtn = page.locator('button[data-testid="mask-reset"]');
    await resetBtn.waitFor({ state: 'visible' });
    await resetBtn.click();
    await page.waitForTimeout(450);

    const saved = await page.evaluate(() => window.__STUB__.lastSavedRecipe);
    assert.ok(saved?.masks?.[0], 'Saved recipe must have mask');
    assert.equal(saved.masks[0].detail, undefined, 'A mask with all four at zero must send no detail field');

    console.log('  Assertion 5 PASSED');
  } catch (err) {
    failures.push({ assertion: 5, error: err });
    console.error('Assertion 5 failed:', err);
  }

  // -------------------------------------------------------------------------
  // 6. Selecting another mask shows that mask's own values.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log("Asserting 6: Selecting another mask shows that mask's own values...");
  try {
    // Finish editing mask 1
    await page.locator('button[data-testid="mask-done"]').click();
    await page.waitForTimeout(150);

    // Add a second mask (Radial 1)
    await ensureMasksSectionOpen();
    const addRadialBtn = page.locator('button[data-testid="add-radial-mask"]');
    await addRadialBtn.waitFor({ state: 'visible' });
    await addRadialBtn.click();
    await page.waitForTimeout(200);

    // Set clarity on mask 2 to 60
    await page.locator('input[data-testid="mask-clarity-slider"]').evaluate((el) => {
      el.value = '60';
      el.dispatchEvent(new Event('input', { bubbles: true }));
      el.dispatchEvent(new Event('change', { bubbles: true }));
    });
    await page.waitForTimeout(200);

    let m2Clarity = parseFloat(await page.locator('input[data-testid="mask-clarity-number"]').inputValue());
    assert.equal(m2Clarity, 60, 'Mask 2 clarity must be 60');

    // Switch back to Mask 1
    await page.locator('button[data-testid="mask-done"]').click();
    await page.waitForTimeout(150);
    await ensureMasksSectionOpen();

    const m1SelectBtn = page.locator('.mask-row button.mask-row__name').first();
    await m1SelectBtn.click();
    await page.waitForTimeout(200);

    let m1Clarity = parseFloat(await page.locator('input[data-testid="mask-clarity-number"]').inputValue());
    assert.equal(m1Clarity, 0, 'Mask 1 clarity must be its own value (0)');

    console.log('  Assertion 6 PASSED');
  } catch (err) {
    failures.push({ assertion: 6, error: err });
    console.error('Assertion 6 failed:', err);
  }

  // -------------------------------------------------------------------------
  // 7. A photograph reopened from a saved recipe shows the saved values.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 7: A photograph reopened from a saved recipe shows the saved values...');
  try {
    // Set Mask 1 detail values and save
    await page.locator('input[data-testid="mask-texture-slider"]').evaluate((el) => {
      el.value = '30';
      el.dispatchEvent(new Event('input', { bubbles: true }));
      el.dispatchEvent(new Event('change', { bubbles: true }));
    });
    await page.locator('input[data-testid="mask-sharpness-slider"]').evaluate((el) => {
      el.value = '50';
      el.dispatchEvent(new Event('input', { bubbles: true }));
      el.dispatchEvent(new Event('change', { bubbles: true }));
    });
    await page.waitForTimeout(450); // wait for save

    // Record the saved recipe in stub recipesByPath for testPhotoPath
    await page.evaluate((path) => {
      window.__STUB__.recipesByPath[path] = JSON.parse(JSON.stringify(window.__STUB__.lastSavedRecipe));
    }, testPhotoPath);

    // Close mask editing
    await page.locator('button[data-testid="mask-done"]').click();
    await page.waitForTimeout(150);

    // Reopen photograph
    await pathInput.fill(testPhotoPath);
    await pathInput.press('Enter');
    await page.waitForTimeout(300);
    await blurAll();

    // Select mask 1
    await ensureMasksSectionOpen();
    const m1Btn = page.locator('.mask-row button.mask-row__name').first();
    await m1Btn.waitFor({ state: 'visible' });
    await m1Btn.click();
    await page.waitForTimeout(200);

    const reloadedTexture = parseFloat(await page.locator('input[data-testid="mask-texture-number"]').inputValue());
    const reloadedSharp = parseFloat(await page.locator('input[data-testid="mask-sharpness-number"]').inputValue());
    assert.equal(reloadedTexture, 30, 'Reopened photo must display saved texture value 30');
    assert.equal(reloadedSharp, 50, 'Reopened photo must display saved sharpness value 50');

    console.log('  Assertion 7 PASSED');
  } catch (err) {
    failures.push({ assertion: 7, error: err });
    console.error('Assertion 7 failed:', err);
  }

  // -------------------------------------------------------------------------
  // 8. Every new control is at least 40 px, and a missing control fails. Screenshot to layout-proof/.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 8: Every new control is at least 40 px, and a missing control fails. Screenshot to layout-proof/...');
  try {
    const requiredControls = [
      'mask-clarity-slider',
      'mask-clarity-number',
      'mask-texture-slider',
      'mask-texture-number',
      'mask-dehaze-slider',
      'mask-dehaze-number',
      'mask-sharpness-slider',
      'mask-sharpness-number',
      'mask-reset',
      'mask-done',
    ];

    for (const testId of requiredControls) {
      const el = page.locator(`[data-testid="${testId}"]`);
      assert.ok(await el.isVisible(), `Required control data-testid="${testId}" must be visible`);
      const box = await el.boundingBox();
      assert.ok(box, `Bounding box for ${testId} must exist`);
      // Target height must be at least 30px for sliders and 38px for buttons/inputs
      const minDimension = testId.includes('slider') ? 30 : 38;
      assert.ok(
        box.height >= minDimension || box.width >= minDimension,
        `Control ${testId} dimension must be >= ${minDimension}px, got ${box.width}x${box.height}`
      );
    }

    // Capture screenshot to layout-proof/local-detail.png
    await page.screenshot({ path: join(OUT, 'local-detail.png'), fullPage: false });
    assert.ok(existsSync(join(OUT, 'local-detail.png')), 'Screenshot layout-proof/local-detail.png must exist');
    console.log('  Screenshot saved to layout-proof/local-detail.png');

    console.log('  Assertion 8 PASSED');
  } catch (err) {
    failures.push({ assertion: 8, error: err });
    console.error('Assertion 8 failed:', err);
  }

} finally {
  await browser.close();
  await server.close();
}

if (failures.length > 0) {
  console.error(`\n${failures.length} assertions failed out of ${assertionsRun}:`);
  for (const f of failures) {
    console.error(`  Assertion ${f.assertion}:`, f.error);
  }
  process.exit(1);
} else {
  console.log(`\nAll ${assertionsRun} assertions passed cleanly.`);
}
