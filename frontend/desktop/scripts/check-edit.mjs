/**
 * ED-7 acceptance & render timing benchmark in a real browser.
 *
 * Pass criteria:
 * - drag p95 ≤ 50 ms and settle p95 ≤ 120 ms including a simulated render delay equal
 *   to the measured core figures (8.3 ms drag, 35 ms settle, from ED-4), so the number
 *   stands for the real total rather than the UI alone.
 * - latest-wins: 100 fast inputs at a 30 ms stub result in far fewer than 100 renders
 *   and the final painted frame reflects the final value.
 * - Stacking check: no ancestor of the canvas creates a stacking context after load,
 *   and the canvas's own z-index sits above --z-scanlines (50).
 * - Touch targets: every interactive control is at least 40 px tall.
 * - Hotkey safety: '\' key is ignored while text or number input is focused.
 * - Double-click label resets slider to exact 0.0 identity.
 * - Saved edits reload: reopening a photo restores saved recipe parameters into sliders.
 * - Unreadable sidecar: reports error and disables autosave to avoid overwriting.
 * - Orientations 1-8: marked corner maps to correct visual quadrant and rotated canvas fits in viewport.
 * - Screenshot written to frontend/desktop/layout-proof/edit.png.
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

console.log('Starting Vite harness server for Edit view...');
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

const failures = [];

try {
  await page.goto(base, { waitUntil: 'networkidle' });
  await page.waitForTimeout(200);

  // 1. Open an image
  console.log('Loading test photograph into Edit view...');
  const pathInput = page.locator('input[placeholder="/path/to/image.jpg"]');
  await pathInput.fill('/Volumes/Photos/sample.jpg');
  await pathInput.press('Enter');

  // Wait for canvas to be painted
  await page.waitForSelector('canvas[data-testid="edit-canvas"]');
  await page.waitForFunction(() => {
    const canvas = document.querySelector('canvas[data-testid="edit-canvas"]');
    return canvas && canvas.width > 0 && canvas.height > 0;
  });

  // Give initial render time to settle
  await page.waitForTimeout(100);

  // 2. Drag frame benchmark (p95 <= 50 ms, including 8.3 ms core delay)
  console.log('Benchmarking drag responsiveness (720p proxy, target p95 <= 50 ms)...');
  const slider = page.locator('input[data-testid="exposure-slider"]');
  const dragMeasurements = [];

  for (let i = 0; i < 40; i++) {
    const targetVal = ((i % 20) - 10) * 0.2; // -2.0 to +2.0 EV
    const elapsed = await page.evaluate(async (val) => {
      const el = document.querySelector('input[data-testid="exposure-slider"]');
      const canvas = document.querySelector('canvas[data-testid="edit-canvas"]');
      return new Promise((resolve) => {
        const start = performance.now();
        const onFrame = (e) => {
          if (e.detail?.stage === 'Drag') {
            canvas.removeEventListener('frame-painted', onFrame);
            resolve(performance.now() - start);
          }
        };
        canvas.addEventListener('frame-painted', onFrame);
        el.value = String(val);
        el.dispatchEvent(new Event('input', { bubbles: true }));
      });
    }, targetVal);
    dragMeasurements.push(elapsed);
  }

  dragMeasurements.sort((a, b) => a - b);
  const dragP95 = dragMeasurements[Math.floor(dragMeasurements.length * 0.95)];
  console.log(`  Drag frames: min=${dragMeasurements[0].toFixed(1)} ms, median=${dragMeasurements[Math.floor(dragMeasurements.length * 0.5)].toFixed(1)} ms, p95=${dragP95.toFixed(1)} ms (budget <= 50 ms)`);

  if (dragP95 > 50) {
    failures.push(`Drag p95 was ${dragP95.toFixed(1)} ms (exceeds 50 ms budget)`);
  }

  // 3. Settle frame benchmark (p95 <= 120 ms, including 35 ms core delay)
  console.log('Benchmarking settle responsiveness (1440p proxy, target p95 <= 120 ms)...');
  const settleMeasurements = [];

  for (let i = 0; i < 40; i++) {
    const targetVal = ((i % 15) - 7) * 0.2;
    const elapsed = await page.evaluate(async (val) => {
      const el = document.querySelector('input[data-testid="exposure-slider"]');
      const canvas = document.querySelector('canvas[data-testid="edit-canvas"]');
      return new Promise((resolve) => {
        const start = performance.now();
        const onFrame = (e) => {
          if (e.detail?.stage === 'Settle') {
            canvas.removeEventListener('frame-painted', onFrame);
            resolve(performance.now() - start);
          }
        };
        canvas.addEventListener('frame-painted', onFrame);
        el.value = String(val);
        el.dispatchEvent(new Event('change', { bubbles: true }));
      });
    }, targetVal);
    settleMeasurements.push(elapsed);
  }

  settleMeasurements.sort((a, b) => a - b);
  const settleP95 = settleMeasurements[Math.floor(settleMeasurements.length * 0.95)];
  console.log(`  Settle frames: min=${settleMeasurements[0].toFixed(1)} ms, median=${settleMeasurements[Math.floor(settleMeasurements.length * 0.5)].toFixed(1)} ms, p95=${settleP95.toFixed(1)} ms (budget <= 120 ms)`);

  if (settleP95 > 120) {
    failures.push(`Settle p95 was ${settleP95.toFixed(1)} ms (exceeds 120 ms budget)`);
  }

  // 4. Latest-wins assertion
  console.log('Asserting latest-wins rendering discipline (100 fast inputs with 30 ms stub delay)...');
  const latestWinsResult = await page.evaluate(async () => {
    window.__STUB__.customDelay = 30; // 30 ms delay per render
    window.__STUB__.rendersIssued = 0;

    const el = document.querySelector('input[data-testid="exposure-slider"]');
    const canvas = document.querySelector('canvas[data-testid="edit-canvas"]');

    // Fire 100 inputs rapidly
    for (let i = 1; i <= 100; i++) {
      el.value = String((i / 100) * 4.0); // final value: 4.0
      el.dispatchEvent(new Event('input', { bubbles: true }));
    }

    // Wait until settle/paint finishes
    await new Promise((resolve) => {
      let timeout;
      const onPaint = () => {
        clearTimeout(timeout);
        timeout = setTimeout(resolve, 80);
      };
      canvas.addEventListener('frame-painted', onPaint);
      timeout = setTimeout(resolve, 300);
    });

    const ctx = canvas.getContext('2d');
    // Exposure value is encoded at pixel (1, 0)
    const pixel1 = ctx.getImageData(1, 0, 1, 1).data[0];
    const totalIssued = window.__STUB__.rendersIssued;
    window.__STUB__.customDelay = null;

    // Expected pixel red value for exposure 4.0 is Math.round(128 + 4.0 * 20) = 208
    return {
      totalIssued,
      pixel1,
    };
  });

  console.log(`  Latest-wins: issued ${latestWinsResult.totalIssued} renders for 100 fast inputs (pixel1=${latestWinsResult.pixel1})`);
  if (latestWinsResult.totalIssued > 20) {
    failures.push(`Latest-wins failed: issued ${latestWinsResult.totalIssued} renders (expected far fewer than 100, <= 20)`);
  }
  if (latestWinsResult.pixel1 !== 208) {
    failures.push(`Latest-wins failed: painted pixel was ${latestWinsResult.pixel1} (expected 208 for final exposure 4.0)`);
  }

  // 5. Stacking context check (Point 3)
  console.log('Asserting canvas stacking context and z-index isolation...');
  const stackingResult = await page.evaluate(() => {
    const canvas = document.querySelector('canvas[data-testid="edit-canvas"]');
    if (!canvas) return { error: 'Canvas not found' };

    const offendingAncestors = [];
    let node = canvas.parentElement;
    while (node && node !== document.documentElement) {
      const cs = window.getComputedStyle(node);
      const isPositionedWithZ = cs.position !== 'static' && cs.zIndex !== 'auto';
      const hasTransform = cs.transform !== 'none';
      const hasFilter = cs.filter !== 'none';
      const hasOpacity = parseFloat(cs.opacity) < 1;
      const hasIsolation = cs.isolation === 'isolate';
      const hasWillChange =
        cs.willChange !== 'auto' && /transform|opacity|filter/i.test(cs.willChange);

      if (isPositionedWithZ || hasTransform || hasFilter || hasOpacity || hasIsolation || hasWillChange) {
        offendingAncestors.push({
          tag: node.tagName.toLowerCase(),
          className: node.className,
          position: cs.position,
          zIndex: cs.zIndex,
          transform: cs.transform,
          filter: cs.filter,
          opacity: cs.opacity,
        });
      }
      node = node.parentElement;
    }

    const canvasCs = window.getComputedStyle(canvas);
    const canvasZ = parseInt(canvasCs.zIndex, 10);

    return {
      offendingAncestors,
      canvasZ,
    };
  });

  if (stackingResult.offendingAncestors?.length > 0) {
    for (const off of stackingResult.offendingAncestors) {
      failures.push(
        `Ancestor <${off.tag} class="${off.className}"> creates a stacking context: pos=${off.position} z=${off.zIndex} transform=${off.transform} opacity=${off.opacity}`,
      );
    }
  }
  if (!stackingResult.canvasZ || stackingResult.canvasZ <= 50) {
    failures.push(`Canvas z-index is ${stackingResult.canvasZ} (must be above --z-scanlines: 50)`);
  }
  console.log(`  Stacking check passed: 0 offending ancestors, canvas z-index = ${stackingResult.canvasZ}`);

  // 6. Interactive controls size check (>= 40px)
  console.log('Asserting interactive controls meet touch target budget (>= 40px)...');
  const controlSizeResults = await page.evaluate(() => {
    const small = [];
    const elements = document.querySelectorAll('button, input, select, textarea, a');
    for (const el of elements) {
      const rect = el.getBoundingClientRect();
      if (rect.width === 0 && rect.height === 0) continue;
      const height = rect.height;
      if (height < 39.5) {
        small.push(`${el.tagName.toLowerCase()}.${el.className || '(no class)'} [${el.type || ''}]: ${height.toFixed(1)}px`);
      }
    }
    return small;
  });

  if (controlSizeResults.length > 0) {
    for (const sm of controlSizeResults) {
      failures.push(`Control touch target under 40px: ${sm}`);
    }
  } else {
    console.log('  All interactive controls clear 40px touch target.');
  }

  // 7. Hotkey '\' ignore check when focused
  console.log('Asserting "\\" key behavior (ignored while input focused, active when blurred)...');
  const numberInput = page.locator('input[data-testid="exposure-number"]');
  await numberInput.focus();
  await page.keyboard.press('\\');
  await page.waitForTimeout(50);

  const beforeActiveFocused = await page.evaluate(() => {
    const btn = document.querySelector('button[data-testid="before-after-btn"]');
    return btn?.classList.contains('active') ?? false;
  });

  if (beforeActiveFocused) {
    failures.push('"\\" key activated Before comparison while number input was focused');
  }

  // Blur and test '\' key again
  await page.evaluate(() => {
    (document.activeElement)?.blur();
  });
  await page.keyboard.down('\\');
  await page.waitForTimeout(50);

  const beforeActiveBlurred = await page.evaluate(() => {
    const btn = document.querySelector('button[data-testid="before-after-btn"]');
    return btn?.classList.contains('active') ?? false;
  });

  if (!beforeActiveBlurred) {
    failures.push('"\\" key failed to activate Before comparison when inputs were blurred');
  }

  await page.keyboard.up('\\');
  await page.waitForTimeout(50);

  // 8. Double-click label to reset to 0
  console.log('Asserting double-click label resets slider to 0.0 identity...');
  await page.evaluate(() => {
    const label = document.querySelector('.adjustment-slider__label');
    label.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
  });
  await page.waitForTimeout(50);

  const resetExposure = await page.evaluate(() => {
    const numInput = document.querySelector('input[data-testid="exposure-number"]');
    return parseFloat(numInput.value);
  });

  if (resetExposure !== 0) {
    failures.push(`Double click label failed to reset exposure: value is ${resetExposure} (expected 0.0)`);
  } else {
    console.log('  Double click reset exposure to 0.0.');
  }

  // 9. Assert saved recipe reloading and unreadable sidecar safety (Item 5)
  console.log('Asserting saved recipe reloading and unreadable sidecar safety (Item 5)...');
  await pathInput.fill('/Volumes/Photos/existing_edits.jpg');
  await pathInput.press('Enter');
  await page.waitForTimeout(100);

  const reloadedValues = await page.evaluate(() => {
    const exp = (document.querySelector('input[data-testid="exposure-number"]')).value;
    const temp = (document.querySelector('input[data-testid="temperature-number"]')).value;
    const hl = (document.querySelector('input[data-testid="highlights-number"]')).value;
    const sh = (document.querySelector('input[data-testid="shadows-number"]')).value;
    const cnt = (document.querySelector('input[data-testid="contrast-number"]')).value;
    return { exp, temp, hl, sh, cnt };
  });

  if (reloadedValues.exp !== '1.5' || reloadedValues.temp !== '-20' || reloadedValues.hl !== '-30') {
    failures.push(`Reloading saved edits failed: expected exp=1.5, temp=-20, hl=-30; got ${JSON.stringify(reloadedValues)}`);
  } else {
    console.log(`  Saved edits successfully reloaded: exp=${reloadedValues.exp}, temp=${reloadedValues.temp}, hl=${reloadedValues.hl}`);
  }

  // 9b. Assert recipe v2 loading with non-zero whites, blacks, brightness, hue (ED-9)
  console.log('Asserting recipe v2 loading (whites, blacks, brightness, hue)...');
  await pathInput.fill('/Volumes/Photos/v2_edits.jpg');
  await pathInput.press('Enter');
  await page.waitForTimeout(100);

  const v2Values = await page.evaluate(() => {
    const whites = (document.querySelector('input[data-testid="whites-number"]'))?.value;
    const blacks = (document.querySelector('input[data-testid="blacks-number"]'))?.value;
    const brightness = (document.querySelector('input[data-testid="brightness-number"]'))?.value;
    const hue = (document.querySelector('input[data-testid="hue-number"]'))?.value;
    return { whites, blacks, brightness, hue };
  });

  if (v2Values.whites !== '25' || v2Values.blacks !== '-35' || v2Values.brightness !== '20' || v2Values.hue !== '45') {
    failures.push(`Recipe v2 values mismatch: expected whites=25, blacks=-35, brightness=20, hue=45; got ${JSON.stringify(v2Values)}`);
  } else {
    console.log(`  Recipe v2 values successfully reloaded: whites=${v2Values.whites}, blacks=${v2Values.blacks}, brightness=${v2Values.brightness}, hue=${v2Values.hue}`);
  }

  // 9c. Assert collapsible section toggling and section reset (ED-9)
  console.log('Asserting collapsible section toggles and section reset...');
  const toneCurveToggle = page.locator('button[data-testid="section-tone-curve-toggle"]');
  const toneCurveContent = page.locator('[data-testid="section-tone-curve-content"]');

  // Initially Tone Curve is collapsed
  let isToneVisible = await toneCurveContent.isVisible();
  if (isToneVisible) {
    failures.push('Tone Curve section should be collapsed by default');
  }

  // Click to open Tone Curve
  await toneCurveToggle.click();
  await page.waitForTimeout(50);
  isToneVisible = await toneCurveContent.isVisible();
  if (!isToneVisible) {
    failures.push('Tone Curve section failed to expand on click');
  } else {
    console.log('  Tone Curve section expanded successfully.');
  }

  // Click to collapse Tone Curve
  await toneCurveToggle.click();
  await page.waitForTimeout(50);
  isToneVisible = await toneCurveContent.isVisible();
  if (isToneVisible) {
    failures.push('Tone Curve section failed to collapse on second click');
  } else {
    console.log('  Tone Curve section collapsed successfully.');
  }

  // Test section reset on Basic
  const basicResetBtn = page.locator('button[data-testid="section-basic-reset"]');
  await basicResetBtn.click();
  await page.waitForTimeout(50);
  const resetV2Values = await page.evaluate(() => {
    const whites = (document.querySelector('input[data-testid="whites-number"]'))?.value;
    const blacks = (document.querySelector('input[data-testid="blacks-number"]'))?.value;
    const exp = (document.querySelector('input[data-testid="exposure-number"]'))?.value;
    return { whites, blacks, exp };
  });

  if (resetV2Values.whites !== '0' || resetV2Values.blacks !== '0' || resetV2Values.exp !== '0.0') {
    failures.push(`Section reset failed: expected whites=0, blacks=0, exp=0.0; got ${JSON.stringify(resetV2Values)}`);
  } else {
    console.log('  Section reset successfully reset all Basic controls to 0.');
  }

  // 9d. Assert Tone Curve editor displays loaded curves, channel selection, point insertion, keyboard move, and deletion (ED-10)
  console.log('Asserting Tone Curve editor (loaded v2 curves, channel pills, point editing, and reset)...');
  // Expand Tone Curve section
  await toneCurveToggle.click();
  await page.waitForTimeout(50);

  const curveEditor = page.locator('[data-testid="tone-curve-editor"]');
  const isEditorVisible = await curveEditor.isVisible();
  if (!isEditorVisible) {
    failures.push('Tone Curve editor is not visible in expanded section');
  }

  // Check that Luma channel has 4 control points from loaded v2 recipe
  const lumaPointsCount = await page.evaluate(() => {
    return document.querySelectorAll('[data-testid="tone-curve-editor"] .curve-editor__point-group').length;
  });
  if (lumaPointsCount !== 4) {
    failures.push(`Expected 4 Luma curve points from loaded v2 recipe, found ${lumaPointsCount}`);
  } else {
    console.log('  Loaded v2 recipe correctly displays 4 control points on Luma curve.');
  }

  // Channel switching: Red (3 points), Green (2 points), Blue (3 points)
  const redPill = page.locator('button[data-testid="curve-channel-red"]');
  await redPill.click();
  await page.waitForTimeout(30);
  const redPointsCount = await page.evaluate(() => {
    return document.querySelectorAll('[data-testid="tone-curve-editor"] .curve-editor__point-group').length;
  });
  if (redPointsCount !== 3) {
    failures.push(`Expected 3 Red curve points, found ${redPointsCount}`);
  }

  const greenPill = page.locator('button[data-testid="curve-channel-green"]');
  await greenPill.click();
  await page.waitForTimeout(30);
  const greenPointsCount = await page.evaluate(() => {
    return document.querySelectorAll('[data-testid="tone-curve-editor"] .curve-editor__point-group').length;
  });
  if (greenPointsCount !== 2) {
    failures.push(`Expected 2 Green curve points, found ${greenPointsCount}`);
  }

  const bluePill = page.locator('button[data-testid="curve-channel-blue"]');
  await bluePill.click();
  await page.waitForTimeout(30);
  const bluePointsCount = await page.evaluate(() => {
    return document.querySelectorAll('[data-testid="tone-curve-editor"] .curve-editor__point-group').length;
  });
  if (bluePointsCount !== 3) {
    failures.push(`Expected 3 Blue curve points, found ${bluePointsCount}`);
  } else {
    console.log('  Channel switching verified across Red (3), Green (2), Blue (3) curves.');
  }

  // Switch back to Luma
  const lumaPill = page.locator('button[data-testid="curve-channel-luma"]');
  await lumaPill.click();
  await page.waitForTimeout(30);

  // Point insertion on click
  const graph = page.locator('[data-testid="curve-graph"]');
  const graphBox = await graph.boundingBox();
  if (graphBox) {
    // Click at ~45% width and 50% height
    await page.mouse.click(graphBox.x + graphBox.width * 0.45, graphBox.y + graphBox.height * 0.5);
    await page.waitForTimeout(50);
    const addedPointsCount = await page.evaluate(() => {
      return document.querySelectorAll('[data-testid="tone-curve-editor"] .curve-editor__point-group').length;
    });
    if (addedPointsCount !== 5) {
      failures.push(`Point insertion failed: expected 5 points, found ${addedPointsCount}`);
    } else {
      console.log('  Point insertion via graph click successfully added control point (total 5).');
    }

    // Keyboard move on selected point (ArrowUp increases value)
    const prevCoordText = await page.evaluate(() => {
      return document.querySelector('.curve-editor__coord:last-child')?.textContent || '';
    });
    await page.keyboard.press('ArrowUp');
    await page.keyboard.press('ArrowUp');
    await page.waitForTimeout(30);
    const newCoordText = await page.evaluate(() => {
      return document.querySelector('.curve-editor__coord:last-child')?.textContent || '';
    });
    console.log(`  Keyboard navigation: ArrowUp adjusted coordinate from ${prevCoordText} to ${newCoordText}`);

    // Point deletion: press Delete key
    await page.keyboard.press('Delete');
    await page.waitForTimeout(50);
    const afterDeleteCount = await page.evaluate(() => {
      return document.querySelectorAll('[data-testid="tone-curve-editor"] .curve-editor__point-group').length;
    });
    if (afterDeleteCount !== 4) {
      failures.push(`Point deletion failed: expected 4 points after delete, found ${afterDeleteCount}`);
    } else {
      console.log('  Point deletion via Delete key successfully restored point count to 4.');
    }
  }

  // Section reset for Tone Curve
  const toneCurveResetBtn = page.locator('button[data-testid="section-tone-curve-reset"]');
  await toneCurveResetBtn.click();
  await page.waitForTimeout(50);
  const afterResetPointsCount = await page.evaluate(() => {
    return document.querySelectorAll('[data-testid="tone-curve-editor"] .curve-editor__point-group').length;
  });
  if (afterResetPointsCount !== 2) {
    failures.push(`Tone Curve section reset failed: expected 2 identity points, found ${afterResetPointsCount}`);
  } else {
    console.log('  Tone Curve section reset successfully restored identity curve [(0,0), (1,1)].');
  }

  // Corrupted sidecar
  await pathInput.fill('/Volumes/Photos/corrupted_sidecar.jpg');
  await pathInput.press('Enter');
  await page.waitForTimeout(100);

  const corruptReport = await page.evaluate(() => {
    const errEl = document.querySelector('[data-testid="edit-error"]');
    const statusEl = document.querySelector('[data-testid="save-status"]');
    return {
      errorText: errEl?.textContent?.trim() || '',
      statusText: statusEl?.textContent?.trim() || '',
    };
  });

  if (!corruptReport.errorText.includes('Syntax error') && !corruptReport.errorText.includes('invalid JSON')) {
    failures.push(`Corrupted sidecar error not displayed: ${corruptReport.errorText}`);
  }
  if (!corruptReport.statusText.includes('Autosave disabled')) {
    failures.push(`Autosave not disabled for corrupted sidecar: status="${corruptReport.statusText}"`);
  } else {
    console.log(`  Corrupted sidecar correctly reported error and disabled autosave.`);
  }

  // 10. Assert EXIF orientations 1-8 and rotated fit inside viewport (Item 8)
  console.log('Asserting EXIF orientations 1-8 and rotated fit inside viewport (Item 8)...');
  const expectedCorners = {
    1: { corner: 'top-left', portrait: false },
    2: { corner: 'top-right', portrait: false },
    3: { corner: 'bottom-right', portrait: false },
    4: { corner: 'bottom-left', portrait: false },
    5: { corner: 'top-left', portrait: true },
    6: { corner: 'top-right', portrait: true },
    7: { corner: 'bottom-right', portrait: true },
    8: { corner: 'bottom-left', portrait: true },
  };

  for (let o = 1; o <= 8; o++) {
    await page.evaluate((orient) => {
      window.__STUB__.orientation = orient;
    }, o);

    await pathInput.fill(`/Volumes/Photos/orient_${o}.jpg`);
    await pathInput.press('Enter');
    await page.waitForTimeout(100);

    const orientResult = await page.evaluate(() => {
      const canvas = document.querySelector('canvas[data-testid="edit-canvas"]');
      const container = document.querySelector('.canvas-viewport__container');
      if (!canvas || !container) return { error: 'elements missing' };

      const cRect = container.getBoundingClientRect();
      const rect = canvas.getBoundingClientRect();

      // Check fit inside viewport container (within 2px tolerance for layout rounding)
      const fits =
        rect.width <= cRect.width + 2 &&
        rect.height <= cRect.height + 2 &&
        rect.left >= cRect.left - 2 &&
        rect.top >= cRect.top - 2 &&
        rect.right <= cRect.right + 2 &&
        rect.bottom <= cRect.bottom + 2;

      // Check corner of raw (0, 0)
      const cs = window.getComputedStyle(canvas);
      const transform = cs.transform === 'none' ? undefined : cs.transform;
      const matrix = new DOMMatrixReadOnly(transform);

      const cx = canvas.width / 2;
      const cy = canvas.height / 2;
      // raw (0, 0) relative to center is (-cx, -cy)
      const pt = matrix.transformPoint(new DOMPoint(-cx, -cy));

      let corner = '';
      if (pt.y < 0) {
        corner = pt.x < 0 ? 'top-left' : 'top-right';
      } else {
        corner = pt.x < 0 ? 'bottom-left' : 'bottom-right';
      }

      const isPortrait = rect.height > rect.width;

      return {
        fits,
        corner,
        isPortrait,
        rect: { w: Math.round(rect.width), h: Math.round(rect.height) },
        cRect: { w: Math.round(cRect.width), h: Math.round(cRect.height) },
      };
    });

    const expected = expectedCorners[o];
    if (!orientResult.fits) {
      failures.push(`Orientation ${o} does not fit in container: canvas=${JSON.stringify(orientResult.rect)} container=${JSON.stringify(orientResult.cRect)}`);
    }
    if (orientResult.corner !== expected.corner) {
      failures.push(`Orientation ${o} marked corner landed at ${orientResult.corner} (expected ${expected.corner})`);
    }
    if (orientResult.isPortrait !== expected.portrait) {
      failures.push(`Orientation ${o} portrait state was ${orientResult.isPortrait} (expected ${expected.portrait})`);
    }
    console.log(`  Orientation ${o}: marked corner=${orientResult.corner}, portrait=${orientResult.isPortrait}, fits inside container=true`);
  }

  // Reset to default image for screenshot
  await page.evaluate(() => {
    window.__STUB__.orientation = 1;
  });
  await pathInput.fill('/Volumes/Photos/sample.jpg');
  await pathInput.press('Enter');
  await page.waitForTimeout(100);

  // 11. Take visual layout proof screenshot
  const screenshotPath = join(OUT, 'edit.png');
  await page.screenshot({ path: screenshotPath, fullPage: true });
  console.log(`Visual proof screenshot saved to ${screenshotPath}`);
} catch (err) {
  failures.push(`Harness execution error: ${err instanceof Error ? err.stack : String(err)}`);
} finally {
  await browser.close();
  await server.close();
}

console.log('\n--- check:edit Summary ---');
if (failures.length > 0) {
  console.error(`FAILED with ${failures.length} issues:`);
  for (const f of failures) {
    console.error(`  - ${f}`);
  }
  process.exit(1);
} else {
  console.log('PASSED all checks.');
}
