/**
 * RP-11 Masks: Ranges and Combining acceptance & verification in a real headless browser.
 *
 * Asserts all 11 requirements as numbered in the brief:
 * 1. Add Linear mask creates base mask with handles.
 * 2. Add Luminance component (intersect) displays component row with op selector intersect.
 * 3. Change component op to subtract, verify coverage canvas updates (pixel coverage diminishes).
 * 4. Component invert toggle inverts the component's effect.
 * 5. Component delete removes the component.
 * 6. Add Luminance range mask directly via + Luminance range button.
 * 7. Add Colour range mask directly via + Colour range button (eyedropper click).
 * 8. Add Background mask directly via + Background button (inverts subject auto mask).
 * 9. Serialization round-trip: components persist in recipe JSON; older recipes without components load cleanly; mask without components does not serialize components: [].
 * 10. Mask limit (4) still enforced: adding a mask disabled at 4; components within a mask are not counted against the 4-mask limit.
 * 11. Touch targets for all interactive controls (buttons, selects, rows) at least 40px.
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

console.log('Starting Vite harness server for check:masks...');
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

  const ensureMasksSectionOpen = async () => {
    const section = page.locator('[data-testid="section-masks"]');
    await section.waitFor({ state: 'visible' });
    const isOpen = await section.evaluate((el) => el.classList.contains('is-open'));
    if (!isOpen) {
      await page.locator('button[data-testid="section-masks-toggle"]').click();
      await page.waitForTimeout(100);
    }
  };

  // Helper to count non-zero pixels on mask coverage canvas
  const getCoveragePixelCount = async () => {
    return await page.evaluate(() => {
      const canvas = document.querySelector('canvas[data-testid="mask-coverage"]');
      if (!canvas) return 0;
      const ctx = canvas.getContext('2d');
      if (!ctx || canvas.width === 0 || canvas.height === 0) return 0;
      const data = ctx.getImageData(0, 0, canvas.width, canvas.height).data;
      let count = 0;
      for (let i = 3; i < data.length; i += 4) {
        if (data[i] > 0) count++;
      }
      return count;
    });
  };

  // Load photograph
  await page.evaluate(() => {
    window.__STUB__.orientation = 1;
    window.__STUB__.toStoredOverride = null;
    window.__STUB__.renders = [];
    window.__STUB__.lastSavedRecipe = null;
    window.__STUB__.modelsPresent = { subject: true, sky: true };
  });
  await pathInput.fill('/Volumes/Photos/masks_test.jpg');
  await pathInput.press('Enter');
  await page.waitForTimeout(250);
  await blurAll();

  await ensureMasksSectionOpen();

  // -------------------------------------------------------------------------
  // 1. Add Linear mask creates base mask with handles.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 1: Add Linear mask creates base mask with handles...');
  const addLinearBtn = page.locator('button[data-testid="add-linear-mask"]');
  await addLinearBtn.waitFor({ state: 'visible' });
  await addLinearBtn.click();
  await page.waitForTimeout(200);

  const maskEditing = page.locator('[data-testid="mask-editing"]');
  await maskEditing.waitFor({ state: 'visible' });

  const maskEditingName = page.locator('[data-testid="mask-editing-name"]');
  const nameText = await maskEditingName.textContent();
  if (!nameText || !nameText.includes('Linear 1')) {
    failures.push(`Assertion 1: Expected mask editing title to contain 'Linear 1', got '${nameText}'`);
  }

  // Check that handles overlay exists
  const handles = page.locator('svg line, svg circle');
  const handleCount = await handles.count();
  if (handleCount === 0) {
    failures.push('Assertion 1: MaskOverlay handles (line/circle) not found on photograph');
  }

  // -------------------------------------------------------------------------
  // 2. Add Luminance component (intersect) displays component row with op selector intersect.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 2: Add Luminance component (intersect) displays component row with op selector intersect...');
  const addCompLumBtn = page.locator('button[data-testid="add-component-luminance"]');
  await addCompLumBtn.waitFor({ state: 'visible' });
  await addCompLumBtn.click();
  await page.waitForTimeout(250);

  const compRow0 = page.locator('[data-testid="component-row-0"]');
  await compRow0.waitFor({ state: 'visible' });

  const opSelect0 = page.locator('select[data-testid="component-op-0"]');
  await opSelect0.waitFor({ state: 'visible' });
  const opVal = await opSelect0.inputValue();
  if (opVal !== 'intersect') {
    failures.push(`Assertion 2: Expected default component op to be 'intersect', got '${opVal}'`);
  }

  const compSelect0 = page.locator('button[data-testid="component-select-0"]');
  const compLabel = await compSelect0.textContent();
  if (!compLabel || !compLabel.includes('Luminance 25–75%')) {
    failures.push(`Assertion 2: Expected component label 'Luminance 25–75%', got '${compLabel}'`);
  }

  // -------------------------------------------------------------------------
  // 3. Change component op to subtract, verify coverage canvas updates (pixel coverage diminishes).
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 3: Change component op to subtract, verify coverage canvas updates (pixel coverage diminishes)...');
  // Make sure overlay is visible
  const overlayBtn = page.locator('button[data-testid="mask-show-overlay"]');
  const isOverlayOn = await overlayBtn.evaluate((el) => el.classList.contains('active'));
  if (!isOverlayOn) {
    await overlayBtn.click();
    await page.waitForTimeout(200);
  }

  const covIntersect = await getCoveragePixelCount();

  // Change to subtract
  await opSelect0.selectOption('subtract');
  await page.waitForTimeout(300);

  const covSubtract = await getCoveragePixelCount();
  if (covSubtract >= covIntersect) {
    failures.push(`Assertion 3: Expected coverage to diminish with subtract: intersect=${covIntersect}, subtract=${covSubtract}`);
  }

  // -------------------------------------------------------------------------
  // 4. Component invert toggle inverts the component's effect.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 4: Component invert toggle inverts component effect...');
  const invertBtn = page.locator('button[data-testid="component-invert-0"]');
  await invertBtn.waitFor({ state: 'visible' });
  await invertBtn.click();
  await page.waitForTimeout(300);

  const isInverted = await invertBtn.evaluate((el) => el.classList.contains('active'));
  if (!isInverted) {
    failures.push('Assertion 4: Invert button did not show active state');
  }

  const covInverted = await getCoveragePixelCount();
  if (covInverted <= covSubtract) {
    failures.push(`Assertion 4: Inverting subtract component expected coverage to increase/change: before=${covSubtract}, after=${covInverted}`);
  }

  // -------------------------------------------------------------------------
  // 5. Component delete removes the component.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 5: Component delete removes the component...');
  const deleteBtn = page.locator('button[data-testid="component-delete-0"]');
  await deleteBtn.waitFor({ state: 'visible' });
  await deleteBtn.click();
  await page.waitForTimeout(200);

  const rowCountAfterDelete = await page.locator('[data-testid="component-row-0"]').count();
  if (rowCountAfterDelete !== 0) {
    failures.push('Assertion 5: Component row 0 was not removed after clicking delete');
  }

  // Finish editing Linear 1
  await page.locator('button[data-testid="mask-done"]').click();
  await page.waitForTimeout(200);

  // -------------------------------------------------------------------------
  // 6. Add Luminance range mask directly via + Luminance range button.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 6: Add Luminance range mask directly via + Luminance range button...');
  const addLumMaskBtn = page.locator('button[data-testid="add-luminance-mask"]');
  await addLumMaskBtn.waitFor({ state: 'visible' });
  await addLumMaskBtn.click();
  await page.waitForTimeout(200);

  const lumName = await page.locator('[data-testid="mask-editing-name"]').textContent();
  if (!lumName || !lumName.includes('Luminance 1')) {
    failures.push(`Assertion 6: Expected mask name 'Luminance 1', got '${lumName}'`);
  }

  const lumLow = page.locator('input[data-testid="mask-range-low-number"]');
  const lumHigh = page.locator('input[data-testid="mask-range-high-number"]');
  const lumFeather = page.locator('input[data-testid="mask-range-feather-number"]');
  await lumLow.waitFor({ state: 'visible' });
  await lumHigh.waitFor({ state: 'visible' });
  await lumFeather.waitFor({ state: 'visible' });

  const lowVal = await lumLow.inputValue();
  const highVal = await lumHigh.inputValue();
  const featherVal = await lumFeather.inputValue();

  if (lowVal !== '25' || highVal !== '75' || featherVal !== '20') {
    failures.push(`Assertion 6: Luminance defaults expected 25/75/20, got low=${lowVal}, high=${highVal}, feather=${featherVal}`);
  }

  await page.locator('button[data-testid="mask-done"]').click();
  await page.waitForTimeout(200);

  // -------------------------------------------------------------------------
  // 7. Add Colour range mask directly via + Colour range button (eyedropper click).
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 7: Add Colour range mask directly via + Colour range button (eyedropper click)...');
  await page.evaluate(() => {
    window.__STUB__.colourAtCalls = [];
    window.__STUB__.colourAtResult = { hue: 120.0, chroma: 0.25, lightness: 0.5 };
  });

  const addColourMaskBtn = page.locator('button[data-testid="add-colour-mask"]');
  await addColourMaskBtn.waitFor({ state: 'visible' });
  await addColourMaskBtn.click();
  await page.waitForTimeout(100);

  const isEyedropperActive = await page.evaluate(() => {
    return document.querySelector('.canvas-viewport__container')?.classList.contains('is-eyedropper');
  });
  if (!isEyedropperActive) {
    failures.push('Assertion 7: Clicking + Colour range did not activate eyedropper');
  }

  const canvas = page.locator('canvas[data-testid="edit-canvas"]');
  const cbox = await canvas.boundingBox();
  if (cbox) {
    await page.mouse.click(cbox.x + cbox.width * 0.5, cbox.y + cbox.height * 0.5);
    await page.waitForTimeout(300);
  } else {
    failures.push('Assertion 7: Canvas bounding box not found for click');
  }

  const colourMaskName = await page.locator('[data-testid="mask-editing-name"]').textContent();
  if (!colourMaskName || !colourMaskName.includes('Colour Range 1')) {
    failures.push(`Assertion 7: Expected mask name 'Colour Range 1', got '${colourMaskName}'`);
  }

  const colourRangeInput = page.locator('input[data-testid="mask-colour-range-number"]');
  await colourRangeInput.waitFor({ state: 'visible' });
  const colourRangeVal = await colourRangeInput.inputValue();
  if (colourRangeVal !== '50') {
    failures.push(`Assertion 7: Expected colour range slider default 50, got '${colourRangeVal}'`);
  }

  const repickBtn = page.locator('button[data-testid="mask-colour-repick"]');
  if (!(await repickBtn.isVisible())) {
    failures.push('Assertion 7: Re-pick colour button not visible');
  }

  await page.locator('button[data-testid="mask-done"]').click();
  await page.waitForTimeout(200);

  // -------------------------------------------------------------------------
  // 8. Add Background mask directly via + Background button (inverts subject auto mask).
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 8: Add Background mask directly via + Background button (inverts subject auto mask)...');
  const addBgBtn = page.locator('button[data-testid="add-background-mask"]');
  await addBgBtn.waitFor({ state: 'visible' });
  await addBgBtn.click();
  await page.waitForTimeout(300);

  const bgMaskName = await page.locator('[data-testid="mask-editing-name"]').textContent();
  if (!bgMaskName || !bgMaskName.includes('Background')) {
    failures.push(`Assertion 8: Expected mask name 'Background', got '${bgMaskName}'`);
  }

  // Done with Background editing for now to check the 4-mask limit
  await page.locator('button[data-testid="mask-done"]').click();
  await page.waitForTimeout(200);

  // -------------------------------------------------------------------------
  // 10. Mask limit (4) still enforced: adding a mask disabled at 4; components
  //     within a mask are not counted against the 4-mask limit.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 10: Mask limit (4) still enforced; components not counted against 4-mask limit...');
  // We now have 4 masks: Linear 1, Luminance 1, Colour Range 1, Background.
  const addMaskButtons = [
    'add-linear-mask',
    'add-radial-mask',
    'add-brush-mask',
    'add-luminance-mask',
    'add-colour-mask',
    'add-subject-mask',
    'add-sky-mask',
    'add-background-mask',
  ];
  for (const btnId of addMaskButtons) {
    const btn = page.locator(`button[data-testid="${btnId}"]`);
    const isDisabled = await btn.isDisabled();
    if (!isDisabled) {
      failures.push(`Assertion 10: Mask button ${btnId} is not disabled when at limit of 4 masks`);
    }
  }

  // Select Background mask to edit it
  const selectBgMask = page.locator('button[data-testid^="mask-select-"]').last();
  await selectBgMask.click();
  await page.waitForTimeout(200);

  // Component add buttons must NOT be disabled by the 4-mask limit
  const addCompLinear = page.locator('button[data-testid="add-component-linear"]');
  const addCompRadial = page.locator('button[data-testid="add-component-radial"]');
  const addCompLum = page.locator('button[data-testid="add-component-luminance"]');

  const linDisabled = await addCompLinear.isDisabled();
  const radDisabled = await addCompRadial.isDisabled();
  const lumDisabled = await addCompLum.isDisabled();

  if (linDisabled || radDisabled || lumDisabled) {
    failures.push('Assertion 10: Adding components inside a mask was incorrectly disabled by the 4-mask limit');
  }

  // Add a component to verify it succeeds
  await addCompLum.click();
  await page.waitForTimeout(200);
  const compRows = await page.locator('[data-testid^="component-row-"]').count();
  if (compRows === 0) {
    failures.push('Assertion 10: Component could not be added when at 4-mask limit');
  }

  // -------------------------------------------------------------------------
  // 9. Serialization round-trip: components persist in recipe JSON; older recipes without
  //    components load cleanly; mask without components does not serialize components: [].
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 9: Serialization round-trip of components and clean omission when absent...');
  // Set component op to subtract on Background mask
  const bgCompOp = page.locator('select[data-testid="component-op-0"]');
  await bgCompOp.selectOption('subtract');
  try {
    await page.waitForFunction(
      () => {
        const bg = window.__STUB__.lastSavedRecipe?.masks?.find((m) => m.name === 'Background');
        return bg?.components?.[0]?.op === 'subtract';
      },
      { timeout: 3000 },
    );
  } catch {
    // Timeout will be handled by following assertion check
  }

  const savedRecipe = await page.evaluate(() => window.__STUB__.lastSavedRecipe);
  if (!savedRecipe || !savedRecipe.masks) {
    failures.push('Assertion 9: No saved recipe found in stub');
  } else {
    // Check linear mask (index 0) has NO components property
    const linearMask = savedRecipe.masks[0];
    if ('components' in linearMask && linearMask.components !== undefined) {
      failures.push(`Assertion 9: Mask without components serialized 'components' (${JSON.stringify(linearMask.components)}) instead of omitting it`);
    }

    // Check background mask (index 3) has components array with subtract luminance
    const bgMask = savedRecipe.masks.find((m) => m.name === 'Background');
    if (!bgMask || !bgMask.components || bgMask.components.length !== 1) {
      failures.push(`Assertion 9: Background mask components array missing or length != 1: ${JSON.stringify(bgMask?.components)}`);
    } else {
      const comp = bgMask.components[0];
      if (comp.op !== 'subtract' || comp.kind.type !== 'luminance_range') {
        failures.push(`Assertion 9: Expected component { op: 'subtract', kind: { type: 'luminance_range' } }, got ${JSON.stringify(comp)}`);
      }
    }
  }

  // Verify older recipe without components loads cleanly
  await page.evaluate(() => {
    const oldRecipe = {
      version: 2,
      source_sha256: 'test',
      masks: [
        {
          id: 'old_linear',
          name: 'Old Linear',
          kind: { type: 'linear', start: [0.5, 0.1], end: [0.5, 0.5] },
          invert: false,
          opacity: 1,
          enabled: true,
          adjustments: {},
        },
      ],
    };
    window.__STUB__.recipesByPath['/Volumes/Photos/old_photo.jpg'] = oldRecipe;
  });
  await pathInput.fill('/Volumes/Photos/old_photo.jpg');
  await pathInput.press('Enter');
  await page.waitForTimeout(250);
  await blurAll();

  const oldMaskRow = page.locator('[data-testid="mask-row-old_linear"]');
  await oldMaskRow.waitFor({ state: 'visible' });
  const oldMaskName = await page.locator('[data-testid="mask-select-old_linear"]').textContent();
  if (!oldMaskName || !oldMaskName.includes('Old Linear')) {
    failures.push(`Assertion 9: Older recipe without components failed to load cleanly: name='${oldMaskName}'`);
  }

  // -------------------------------------------------------------------------
  // 11. Touch targets for all interactive controls (buttons, selects, rows) at least 40px.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 11: Touch targets for all interactive controls at least 40px...');
  // Open old_linear for editing
  await page.locator('button[data-testid="mask-select-old_linear"]').click();
  await page.waitForTimeout(200);

  const interactiveTestIds = [
    'add-linear-mask',
    'add-radial-mask',
    'add-brush-mask',
    'add-luminance-mask',
    'add-colour-mask',
    'add-background-mask',
    'add-subject-mask',
    'add-sky-mask',
    'add-component-linear',
    'add-component-radial',
    'add-component-luminance',
    'add-component-colour',
    'add-component-subject',
    'add-component-sky',
    'mask-done',
  ];

  for (const tid of interactiveTestIds) {
    const loc = page.locator(`button[data-testid="${tid}"]`).first();
    if (await loc.isVisible()) {
      const box = await loc.boundingBox();
      if (!box || box.width < 39.5 || box.height < 39.5) {
        failures.push(`Assertion 11: Control ${tid} touch target is under 40px (${box?.width?.toFixed(1)}x${box?.height?.toFixed(1)})`);
      }
    }
  }

  // Add a component to check component row controls size
  await page.locator('button[data-testid="add-component-luminance"]').click();
  await page.waitForTimeout(200);

  const compTestIds = [
    'component-row-0',
    'component-select-0',
    'component-op-0',
    'component-invert-0',
    'component-delete-0',
  ];
  for (const tid of compTestIds) {
    const loc = page.locator(`[data-testid="${tid}"]`).first();
    if (await loc.isVisible()) {
      const box = await loc.boundingBox();
      if (!box || box.width < 39.5 || box.height < 39.5) {
        failures.push(`Assertion 11: Control ${tid} touch target is under 40px (${box?.width?.toFixed(1)}x${box?.height?.toFixed(1)})`);
      }
    }
  }

  const proofPath = join(OUT, 'mask-components.png');
  await page.screenshot({ path: proofPath, fullPage: true });
  console.log(`  Proof screenshot saved to ${proofPath}`);

} catch (err) {
  failures.push(`Unexpected error during check:masks execution: ${err.message}\n${err.stack}`);
} finally {
  await browser.close();
  await server.close();
}

console.log('\n--- check:masks Summary ---');
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
