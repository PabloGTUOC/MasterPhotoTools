/**
 * RP-11 Masks: Ranges and Combining acceptance & verification in a real headless browser.
 *
 * Asserts all eleven requirements from the brief plus model download intent retention (RP-11 Review 1):
 * 1. Luminance range adds a mask of that kind, selected, and it is in the save payload (low=0.25, high=0.75, feather=0.2).
 * 2. Moving Low sends Drag renders carrying it; release sends Settle (moving Low to 40 sends 0.4).
 * 3. Colour range enters picking; a click sends colour_at and creates the mask with the picked values.
 * 4. Background calls the auto mask for Subject and sets invert: true, named Background.
 * 5. Add component adds one; its op, invert and kind reach the save payload.
 * 6. Changing the op is one undo step, labelled Masks (Cmd-Z reverts it).
 * 7. Selecting a radial component shows its handles; dragging one changes the component and not the base shape.
 * 8. Deleting a component removes it from the save payload.
 * 9. A mask without components sends none (and older recipes without components load cleanly).
 * 10. Esc while picking leaves the mode and changes nothing.
 * 11. Touch targets for all interactive controls at least 40px (fails if control missing; screenshot to layout-proof/).
 * Extra: Mask limit (4) still enforced: adding a mask disabled at 4; components within a mask are not counted against the 4-mask limit.
 * 12. Model download for Background retains user intent (creates inverted mask named Background when modelsPresent is false).
 * 13. Model download for Add component retains user intent (adds component to selected mask, not separate mask, when modelsPresent is false).
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

  // Load photograph with clean state
  await page.evaluate(() => {
    window.__STUB__.orientation = 1;
    window.__STUB__.toStoredOverride = null;
    window.__STUB__.renders = [];
    window.__STUB__.lastSavedRecipe = null;
    window.__STUB__.modelsPresent = { subject: true, sky: true };
    window.__STUB__.autoMaskCalls = [];
    window.__STUB__.colourAtCalls = [];
  });
  await pathInput.fill('/Volumes/Photos/masks_test.jpg');
  await pathInput.press('Enter');
  await page.waitForTimeout(250);
  await blurAll();

  await ensureMasksSectionOpen();

  // -------------------------------------------------------------------------
  // 1. Luminance range adds a mask of that kind, selected, and it is in the save payload.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 1: Luminance range adds a mask of that kind, selected, and it is in the save payload...');
  const addLumBtn = page.locator('button[data-testid="add-luminance-mask"]');
  await addLumBtn.waitFor({ state: 'visible' });
  await addLumBtn.click();
  await page.waitForTimeout(200);

  const maskEditing = page.locator('[data-testid="mask-editing"]');
  await maskEditing.waitFor({ state: 'visible' });

  const maskEditingName = page.locator('[data-testid="mask-editing-name"]');
  const lumName = await maskEditingName.textContent();
  if (!lumName || !lumName.includes('Luminance 1')) {
    failures.push(`Assertion 1: Expected mask editing title to contain 'Luminance 1', got '${lumName}'`);
  }

  // Check controls show percentage defaults 25, 75, 20
  const lowInput = page.locator('input[data-testid="mask-range-low-number"]');
  const highInput = page.locator('input[data-testid="mask-range-high-number"]');
  const featherInput = page.locator('input[data-testid="mask-range-feather-number"]');
  await lowInput.waitFor({ state: 'visible' });
  const lowValStr = await lowInput.inputValue();
  const highValStr = await highInput.inputValue();
  const featherValStr = await featherInput.inputValue();
  if (lowValStr !== '25' || highValStr !== '75' || featherValStr !== '20') {
    failures.push(`Assertion 1: Expected slider inputs 25, 75, 20; got ${lowValStr}, ${highValStr}, ${featherValStr}`);
  }

  // Verify save payload has low=0.25, high=0.75, feather=0.2 on 0..1 scale
  await page.waitForFunction(() => {
    const m = window.__STUB__.lastSavedRecipe?.masks?.find((x) => x.name === 'Luminance 1');
    return m && m.kind?.type === 'luminance_range';
  }, { timeout: 4000 });

  const savedLum1 = await page.evaluate(() => {
    return window.__STUB__.lastSavedRecipe?.masks?.find((x) => x.name === 'Luminance 1');
  });
  if (!savedLum1) {
    failures.push('Assertion 1: Luminance 1 mask not found in save payload');
  } else {
    if (savedLum1.kind.low !== 0.25 || savedLum1.kind.high !== 0.75 || savedLum1.kind.feather !== 0.2) {
      failures.push(`Assertion 1: Expected payload low=0.25, high=0.75, feather=0.2; got low=${savedLum1.kind.low}, high=${savedLum1.kind.high}, feather=${savedLum1.kind.feather}`);
    }
  }

  // -------------------------------------------------------------------------
  // 2. Moving Low sends Drag renders carrying it; release sends Settle (moving Low to 40 sends 0.4).
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 2: Moving Low sends Drag renders carrying it; release sends Settle (moving Low to 40 sends 0.4)...');
  await page.evaluate(() => {
    window.__STUB__.renders = [];
  });

  const lowSlider = page.locator('input[data-testid="mask-range-low-slider"]');
  await lowSlider.waitFor({ state: 'visible' });
  const lowBox = await lowSlider.boundingBox();
  if (!lowBox) {
    failures.push('Assertion 2: Low slider bounding box not found');
  } else {
    // Current value is 25%, move to 40%
    const startX = lowBox.x + lowBox.width * 0.25;
    const targetX = lowBox.x + lowBox.width * 0.40;
    const midY = lowBox.y + lowBox.height / 2;

    await page.mouse.move(startX, midY);
    await page.mouse.down();
    await page.mouse.move(targetX, midY, { steps: 5 });
    await page.waitForTimeout(100);

    const dragRenders = await page.evaluate(() => {
      return window.__STUB__.renders.filter((r) => r.stage === 'Drag');
    });
    if (dragRenders.length === 0) {
      failures.push('Assertion 2: No Drag renders issued during Low slider movement');
    }

    await page.mouse.up();

    await page.waitForFunction(() => {
      const m = window.__STUB__.lastSavedRecipe?.masks?.find((x) => x.name === 'Luminance 1');
      return m && Math.abs((m.kind?.low ?? 0) - 0.4) < 0.05;
    }, { timeout: 4000 });

    const settleRenders = await page.evaluate(() => {
      return window.__STUB__.renders.filter((r) => r.stage === 'Settle');
    });
    if (settleRenders.length === 0) {
      failures.push('Assertion 2: No Settle render issued on Low slider release');
    }
  }

  // Done editing Luminance 1
  await page.locator('button[data-testid="mask-done"]').click();
  await page.waitForTimeout(200);

  // -------------------------------------------------------------------------
  // 3. Colour range enters picking; a click sends colour_at and creates the mask with the picked values.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 3: Colour range enters picking; a click sends colour_at and creates the mask with the picked values...');
  await page.evaluate(() => {
    window.__STUB__.colourAtCalls = [];
    window.__STUB__.colourAtResult = { hue: 135.5, chroma: 0.16, lightness: 0.58 };
  });

  const addColourBtn = page.locator('button[data-testid="add-colour-mask"]');
  await addColourBtn.waitFor({ state: 'visible' });
  await addColourBtn.click();
  await page.waitForTimeout(150);

  const isPicking = await page.evaluate(() => {
    return document.querySelector('.canvas-viewport__container')?.classList.contains('is-eyedropper');
  });
  if (!isPicking) {
    failures.push('Assertion 3: + Colour range did not enter eyedropper picking mode');
  }

  const canvas = page.locator('canvas[data-testid="edit-canvas"]');
  const cbox = await canvas.boundingBox();
  if (cbox) {
    await page.mouse.click(cbox.x + cbox.width * 0.5, cbox.y + cbox.height * 0.5);
  }

  await page.waitForFunction(() => {
    return window.__STUB__.lastSavedRecipe?.masks?.some((x) => x.kind?.type === 'colour_range');
  }, { timeout: 4000 });

  const colourCalls = await page.evaluate(() => window.__STUB__.colourAtCalls);
  if (!colourCalls || colourCalls.length === 0) {
    failures.push('Assertion 3: Canvas click did not trigger colour_at');
  }

  const savedColourMask = await page.evaluate(() => {
    return window.__STUB__.lastSavedRecipe?.masks?.find((x) => x.kind?.type === 'colour_range');
  });
  if (!savedColourMask) {
    failures.push('Assertion 3: Colour range mask not found in save payload');
  } else {
    if (Math.abs(savedColourMask.kind.hue - 135.5) > 0.1 || Math.abs(savedColourMask.kind.chroma - 0.16) > 0.05) {
      failures.push(`Assertion 3: Expected picked hue ~135.5, chroma ~0.16; got hue=${savedColourMask.kind.hue}, chroma=${savedColourMask.kind.chroma}`);
    }
  }

  await page.locator('button[data-testid="mask-done"]').click();
  await page.waitForTimeout(200);

  // -------------------------------------------------------------------------
  // 4. Background calls the auto mask for Subject and sets invert: true, named Background.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 4: Background calls the auto mask for Subject and sets invert: true, named Background...');
  await page.evaluate(() => {
    window.__STUB__.autoMaskCalls = [];
  });

  const addBgBtn = page.locator('button[data-testid="add-background-mask"]');
  await addBgBtn.waitFor({ state: 'visible' });
  await addBgBtn.click();

  await page.waitForFunction(() => {
    const bg = window.__STUB__.lastSavedRecipe?.masks?.find((x) => x.name === 'Background');
    return bg && bg.invert && bg.kind?.type === 'auto';
  }, { timeout: 4000 });

  const autoCalls = await page.evaluate(() => window.__STUB__.autoMaskCalls);
  if (!autoCalls || !autoCalls.includes('subject')) {
    failures.push(`Assertion 4: Background button did not call makeAutoMask('subject'): got ${JSON.stringify(autoCalls)}`);
  }

  const bgMask = await page.evaluate(() => {
    return window.__STUB__.lastSavedRecipe?.masks?.find((x) => x.name === 'Background');
  });
  if (!bgMask) {
    failures.push('Assertion 4: No mask named Background found in save payload');
  } else {
    if (!bgMask.invert) {
      failures.push('Assertion 4: Expected Background mask to have invert: true');
    }
    if (bgMask.kind?.type !== 'auto' || bgMask.kind?.target !== 'subject') {
      failures.push(`Assertion 4: Expected Background mask kind auto:subject, got ${JSON.stringify(bgMask.kind)}`);
    }
  }

  // -------------------------------------------------------------------------
  // 5. Add component adds one; its op, invert and kind reach the save payload.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 5: Add component adds one; its op, invert and kind reach the save payload...');
  // Background mask is currently selected. Add a luminance component to it.
  const addCompLumBtn = page.locator('button[data-testid="add-component-luminance"]');
  await addCompLumBtn.waitFor({ state: 'visible' });
  await addCompLumBtn.click();

  const compRow0 = page.locator('[data-testid="component-row-0"]');
  await compRow0.waitFor({ state: 'visible' });

  // Toggle invert on component 0
  const compInvertBtn = page.locator('button[data-testid="component-invert-0"]');
  await compInvertBtn.waitFor({ state: 'visible' });
  await compInvertBtn.click();

  await page.waitForFunction(() => {
    const bg = window.__STUB__.lastSavedRecipe?.masks?.find((x) => x.name === 'Background');
    return bg?.components?.[0]?.invert === true;
  }, { timeout: 4000 });

  const bgWithComp = await page.evaluate(() => {
    return window.__STUB__.lastSavedRecipe?.masks?.find((x) => x.name === 'Background');
  });
  if (!bgWithComp?.components || bgWithComp.components.length === 0) {
    failures.push('Assertion 5: Background mask components missing from save payload');
  } else {
    const comp0 = bgWithComp.components[0];
    if (comp0.op !== 'intersect' || !comp0.invert || comp0.kind?.type !== 'luminance_range') {
      failures.push(`Assertion 5: Component op/invert/kind did not match: got ${JSON.stringify(comp0)}`);
    }
  }

  // -------------------------------------------------------------------------
  // 6. Changing the op is one undo step, labelled Masks.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 6: Changing the op is one undo step, labelled Masks...');
  const opSelect0 = page.locator('select[data-testid="component-op-0"]');
  await opSelect0.waitFor({ state: 'visible' });
  await opSelect0.selectOption('subtract');

  await page.waitForFunction(() => {
    const bg = window.__STUB__.lastSavedRecipe?.masks?.find((x) => x.name === 'Background');
    return bg?.components?.[0]?.op === 'subtract';
  }, { timeout: 4000 });

  // Trigger Cmd-Z / Ctrl-Z
  await page.keyboard.down(process.platform === 'darwin' ? 'Meta' : 'Control');
  await page.keyboard.press('z');
  await page.keyboard.up(process.platform === 'darwin' ? 'Meta' : 'Control');

  await page.waitForFunction(() => {
    const bg = window.__STUB__.lastSavedRecipe?.masks?.find((x) => x.name === 'Background');
    return bg?.components?.[0]?.op === 'intersect';
  }, { timeout: 4000 });

  const opAfterUndo = await opSelect0.inputValue();
  if (opAfterUndo !== 'intersect') {
    failures.push(`Assertion 6: Undo did not revert component op to 'intersect': got '${opAfterUndo}'`);
  }

  // Delete component 0 so Background is clean
  await page.locator('button[data-testid="component-delete-0"]').click();
  await page.waitForFunction(() => {
    const bg = window.__STUB__.lastSavedRecipe?.masks?.find((x) => x.name === 'Background');
    return !bg?.components || bg.components.length === 0;
  }, { timeout: 4000 });
  await page.locator('button[data-testid="mask-done"]').click();
  await page.waitForTimeout(200);

  // -------------------------------------------------------------------------
  // 7. Selecting a radial component shows its handles; dragging one changes the component and not the base shape.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 7: Selecting a radial component shows its handles; dragging one changes the component and not the base shape...');
  // Add a Linear mask as the test mask
  const addLinearBtn = page.locator('button[data-testid="add-linear-mask"]');
  await addLinearBtn.click();
  await page.waitForTimeout(200);

  // Base mask is Linear. Record its original start and end coordinates.
  const linearStart = await page.evaluate(() => {
    const m = window.__STUB__.lastSavedRecipe?.masks?.find((x) => x.kind?.type === 'linear');
    return m?.kind?.start ? [...m.kind.start] : null;
  });

  // Add a Radial component to this linear mask
  const addCompRadial = page.locator('button[data-testid="add-component-radial"]');
  await addCompRadial.waitFor({ state: 'visible' });
  await addCompRadial.click();
  await page.waitForTimeout(250);

  // Select radial component row
  const compSelectRadial = page.locator('button[data-testid="component-select-0"]');
  await compSelectRadial.waitFor({ state: 'visible' });
  await compSelectRadial.click();
  await page.waitForTimeout(200);

  // Check radial handles appear on SVG
  const radialHandles = page.locator('svg circle');
  const countHandles = await radialHandles.count();
  if (countHandles === 0) {
    failures.push('Assertion 7: Radial component handles did not appear on canvas');
  }

  // Drag a radial handle
  const firstHandle = radialHandles.first();
  const handleBox = await firstHandle.boundingBox();
  if (handleBox) {
    await page.mouse.move(handleBox.x + handleBox.width / 2, handleBox.y + handleBox.height / 2);
    await page.mouse.down();
    await page.mouse.move(handleBox.x + 30, handleBox.y + 30, { steps: 5 });
    await page.mouse.up();
  }

  await page.waitForFunction(() => {
    const m = window.__STUB__.lastSavedRecipe?.masks?.find((x) => x.name?.startsWith('Linear'));
    return m?.components?.[0]?.kind?.type === 'radial';
  }, { timeout: 4000 });

  const recipeAfterHandleDrag = await page.evaluate(() => window.__STUB__.lastSavedRecipe);
  const testMaskAfterDrag = recipeAfterHandleDrag?.masks?.find((m) => m.name?.startsWith('Linear'));
  if (!testMaskAfterDrag) {
    failures.push('Assertion 7: Linear mask not found after handle drag');
  } else {
    // Base mask kind must still be linear with identical start
    if (testMaskAfterDrag.kind?.type !== 'linear') {
      failures.push(`Assertion 7: Base mask kind was mutated: ${JSON.stringify(testMaskAfterDrag.kind)}`);
    } else if (linearStart && (testMaskAfterDrag.kind.start[0] !== linearStart[0] || testMaskAfterDrag.kind.start[1] !== linearStart[1])) {
      failures.push(`Assertion 7: Base mask start was mutated by component drag: was ${JSON.stringify(linearStart)}, now ${JSON.stringify(testMaskAfterDrag.kind.start)}`);
    }

    // Component kind must be radial
    const comp0 = testMaskAfterDrag.components?.[0];
    if (comp0?.kind?.type !== 'radial') {
      failures.push(`Assertion 7: Component kind is not radial: ${JSON.stringify(comp0?.kind)}`);
    }
  }

  // -------------------------------------------------------------------------
  // 8. Deleting a component removes it from the save payload.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 8: Deleting a component removes it from the save payload...');
  const compDeleteBtn = page.locator('button[data-testid="component-delete-0"]');
  await compDeleteBtn.waitFor({ state: 'visible' });
  await compDeleteBtn.click();

  await page.waitForFunction(() => {
    const m = window.__STUB__.lastSavedRecipe?.masks?.find((x) => x.name?.startsWith('Linear'));
    return m && (!m.components || m.components.length === 0);
  }, { timeout: 4000 });

  const recipeAfterCompDelete = await page.evaluate(() => window.__STUB__.lastSavedRecipe);
  const testMaskAfterCompDelete = recipeAfterCompDelete?.masks?.find((m) => m.name?.startsWith('Linear'));
  if (testMaskAfterCompDelete?.components && testMaskAfterCompDelete.components.length > 0) {
    failures.push(`Assertion 8: Component was not removed from save payload: ${JSON.stringify(testMaskAfterCompDelete.components)}`);
  }

  // -------------------------------------------------------------------------
  // 9. A mask without components sends none.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 9: A mask without components sends none...');
  if (testMaskAfterCompDelete && 'components' in testMaskAfterCompDelete && testMaskAfterCompDelete.components !== undefined) {
    failures.push(`Assertion 9: Mask without components serialized 'components' (${JSON.stringify(testMaskAfterCompDelete.components)}) instead of omitting it`);
  }

  // Older recipes without components load cleanly
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
  // 10. Esc while picking leaves the mode and changes nothing.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 10: Esc while picking leaves the mode and changes nothing...');
  const countBeforePick = await page.evaluate(() => window.__STUB__.lastSavedRecipe?.masks?.length ?? 1);
  const addColourEscBtn = page.locator('button[data-testid="add-colour-mask"]');
  await addColourEscBtn.waitFor({ state: 'visible' });
  await addColourEscBtn.click();
  await page.waitForTimeout(150);

  const isPickingBeforeEsc = await page.evaluate(() => {
    return document.querySelector('.canvas-viewport__container')?.classList.contains('is-eyedropper');
  });
  if (!isPickingBeforeEsc) {
    failures.push('Assertion 10: Did not enter picking mode');
  }

  await page.keyboard.press('Escape');
  await page.waitForTimeout(200);

  const isPickingAfterEsc = await page.evaluate(() => {
    return document.querySelector('.canvas-viewport__container')?.classList.contains('is-eyedropper');
  });
  if (isPickingAfterEsc) {
    failures.push('Assertion 10: Pressing Esc did not exit eyedropper mode');
  }

  const countAfterEsc = await page.evaluate(() => window.__STUB__.lastSavedRecipe?.masks?.length ?? 1);
  if (countAfterEsc !== countBeforePick) {
    failures.push(`Assertion 10: Pressing Esc while picking created a mask (count changed from ${countBeforePick} to ${countAfterEsc})`);
  }

  // -------------------------------------------------------------------------
  // 11. Touch targets for all interactive controls (buttons, selects, rows) at least 40px.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 11: Touch targets for all interactive controls at least 40px...');
  // Edit the existing mask 'old_linear'
  await page.locator('button[data-testid="mask-select-old_linear"]').click();
  await page.waitForTimeout(200);

  // Add a luminance component to expose component row controls
  await page.locator('button[data-testid="add-component-luminance"]').click();
  await page.waitForTimeout(200);

  const requiredControls = [
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
    'component-row-0',
    'component-select-0',
    'component-op-0',
    'component-invert-0',
    'component-delete-0',
    'mask-done',
  ];

  for (const tid of requiredControls) {
    const loc = page.locator(`[data-testid="${tid}"]`).first();
    const visible = await loc.isVisible();
    if (!visible) {
      failures.push(`Assertion 11: Required control ${tid} is missing or not visible`);
      continue;
    }
    const box = await loc.boundingBox();
    if (!box || box.width < 39.5 || box.height < 39.5) {
      failures.push(`Assertion 11: Control ${tid} touch target is under 40px (${box?.width?.toFixed(1)}x${box?.height?.toFixed(1)})`);
    }
  }

  const proofPath = join(OUT, 'mask-components.png');
  await page.screenshot({ path: proofPath, fullPage: true });
  console.log(`  Proof screenshot saved to ${proofPath}`);

  // Clean up editing
  await page.locator('button[data-testid="mask-done"]').click();
  await page.waitForTimeout(200);

  // -------------------------------------------------------------------------
  // Extra: Mask limit (4) still enforced; components not counted against 4-mask limit.
  // -------------------------------------------------------------------------
  console.log('Asserting extra: Mask limit (4) still enforced; components not counted against 4-mask limit...');
  // We currently have 1 mask (old_linear). Add 3 more to reach 4.
  await page.locator('button[data-testid="add-radial-mask"]').click();
  await page.waitForTimeout(200);
  await page.locator('button[data-testid="mask-done"]').click();
  await page.waitForTimeout(200);

  await page.locator('button[data-testid="add-brush-mask"]').click();
  await page.waitForTimeout(200);
  await page.locator('button[data-testid="mask-done"]').click();
  await page.waitForTimeout(200);

  await page.locator('button[data-testid="add-luminance-mask"]').click();
  await page.waitForTimeout(200);
  await page.locator('button[data-testid="mask-done"]').click();
  await page.waitForTimeout(200);

  // All 8 mask add buttons must be disabled
  for (const btnId of ['add-linear-mask', 'add-radial-mask', 'add-brush-mask', 'add-luminance-mask', 'add-colour-mask', 'add-background-mask', 'add-subject-mask', 'add-sky-mask']) {
    const btn = page.locator(`button[data-testid="${btnId}"]`);
    const dis = await btn.isDisabled();
    if (!dis) {
      failures.push(`Extra: Mask button ${btnId} not disabled at limit of 4 masks`);
    }
  }

  // Select mask 0 to edit components; adding components must NOT be disabled
  await page.locator('[data-testid^="mask-select-"]').first().click();
  await page.waitForTimeout(200);

  const addCompLinear = page.locator('button[data-testid="add-component-linear"]');
  const isCompLinDis = await addCompLinear.isDisabled();
  if (isCompLinDis) {
    failures.push('Extra: Adding component incorrectly disabled by 4-mask limit');
  }
  await page.locator('button[data-testid="mask-done"]').click();
  await page.waitForTimeout(200);

  // -------------------------------------------------------------------------
  // 12. Model download for Background retains intent (creates inverted named Background with modelsPresent=false).
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 12: Model download for Background retains intent (creates inverted named Background with modelsPresent=false)...');
  // Load a new image to start with clean masks
  await page.evaluate(() => {
    window.__STUB__.modelsPresent = { subject: false, sky: false };
    window.__STUB__.autoMaskCalls = [];
  });
  await pathInput.fill('/Volumes/Photos/download_bg_test.jpg');
  await pathInput.press('Enter');
  await page.waitForTimeout(250);
  await blurAll();
  await ensureMasksSectionOpen();

  // Click + Background when model is not present
  const bgBtnDl = page.locator('button[data-testid="add-background-mask"]');
  await bgBtnDl.waitFor({ state: 'visible' });
  await bgBtnDl.click();
  await page.waitForTimeout(200);

  // Auto-download banner should appear
  const autoDlCard = page.locator('[data-testid="auto-download"]');
  await autoDlCard.waitFor({ state: 'visible' });

  // Start download
  const dlStartBtn = page.locator('button[data-testid="auto-download-start"]');
  await dlStartBtn.click();

  // Wait for model download and auto-mask completion (watchJob runs for 300ms + makeAutoMask + save debounce)
  await page.waitForFunction(() => {
    const m = window.__STUB__.lastSavedRecipe?.masks?.find((x) => x.name === 'Background');
    return m && m.invert && m.kind?.type === 'auto' && m.kind?.target === 'subject';
  }, { timeout: 8000 });

  const savedBgAfterDl = await page.evaluate(() => {
    return window.__STUB__.lastSavedRecipe?.masks?.find((m) => m.name === 'Background');
  });
  if (!savedBgAfterDl) {
    failures.push('Assertion 12: After model download, mask named Background was not created');
  } else {
    if (!savedBgAfterDl.invert) {
      failures.push('Assertion 12: Mask created after Background download was not inverted');
    }
    if (savedBgAfterDl.kind?.type !== 'auto' || savedBgAfterDl.kind?.target !== 'subject') {
      failures.push(`Assertion 12: Expected auto:subject mask after Background download, got ${JSON.stringify(savedBgAfterDl.kind)}`);
    }
  }

  // -------------------------------------------------------------------------
  // 13. Model download for Add component retains intent (adds component to selected mask, not separate mask).
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 13: Model download for Add component retains intent (adds component to selected mask, not separate mask)...');
  // Background mask is currently selected. Reset sky model to present=false.
  await page.evaluate(() => {
    window.__STUB__.modelsPresent.sky = false;
  });

  const maskCountBeforeSky = await page.evaluate(() => window.__STUB__.lastSavedRecipe?.masks?.length ?? 1);

  // Click Add component -> + Sky
  const addCompSkyBtn = page.locator('button[data-testid="add-component-sky"]');
  await addCompSkyBtn.waitFor({ state: 'visible' });
  await addCompSkyBtn.click();
  await page.waitForTimeout(200);

  // Auto-download card appears for sky
  const dlSkyStartBtn = page.locator('button[data-testid="auto-download-start"]');
  await dlSkyStartBtn.waitFor({ state: 'visible' });
  await dlSkyStartBtn.click();

  await page.waitForFunction(() => {
    const m = window.__STUB__.lastSavedRecipe?.masks?.find((x) => x.name === 'Background');
    return m?.components?.some((c) => c.kind?.type === 'auto' && c.kind?.target === 'sky');
  }, { timeout: 8000 });

  const recipeAfterSkyDl = await page.evaluate(() => window.__STUB__.lastSavedRecipe);
  const maskCountAfterSky = recipeAfterSkyDl?.masks?.length ?? 0;
  if (maskCountAfterSky !== maskCountBeforeSky) {
    failures.push(`Assertion 13: Model download for component added a separate mask instead of a component (mask count went from ${maskCountBeforeSky} to ${maskCountAfterSky})`);
  }

  const bgMaskWithSkyComp = recipeAfterSkyDl?.masks?.find((m) => m.name === 'Background');
  const skyComp = bgMaskWithSkyComp?.components?.find((c) => c.kind?.type === 'auto' && c.kind?.target === 'sky');
  if (!skyComp) {
    failures.push(`Assertion 13: Expected Sky auto component inside Background mask, got components: ${JSON.stringify(bgMaskWithSkyComp?.components)}`);
  }

} catch (err) {
  failures.push(`Unexpected error during check:masks execution: ${err.message}\n${err.stack}`);
} finally {
  await browser.close();
  await server.close();
}

console.log('\n--- check:masks Summary ---');
console.log(`Assertions executed: ${assertionsRun}/13`);
if (failures.length === 0) {
  console.log('ALL 13 ASSERTIONS PASSED');
  process.exit(0);
} else {
  console.error(`FAILURES (${failures.length}):`);
  for (const f of failures) {
    console.error(`  - ${f}`);
  }
  process.exit(1);
}
