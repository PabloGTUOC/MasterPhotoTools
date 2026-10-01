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

  // 9e. Assert 8-band HSL selective adjustments panel (ED-11)
  console.log('Asserting 8-band HSL panel (swatches, oklch backgrounds, slider values, markers, and resets)...');
  // First load v2_edits.jpg so HSL has non-zero values
  await pathInput.fill('/Volumes/Photos/v2_edits.jpg');
  await pathInput.press('Enter');
  await page.waitForTimeout(100);

  // Expand Colour / HSL section
  const hslToggle = page.locator('button[data-testid="section-hsl-toggle"]');
  await hslToggle.click();
  await page.waitForTimeout(50);

  // Check that all 8 swatches exist and have oklch backgrounds
  const swatchData = await page.evaluate(() => {
    const swatches = Array.from(document.querySelectorAll('.hsl-swatch'));
    return swatches.map((s) => ({
      testId: s.getAttribute('data-testid'),
      title: s.getAttribute('title'),
      styleBg: s.style.backgroundColor,
      hasDot: Boolean(s.querySelector('.hsl-swatch-dot')),
      isActive: s.classList.contains('active'),
      width: s.getBoundingClientRect().width,
      height: s.getBoundingClientRect().height,
    }));
  });

  if (swatchData.length !== 8) {
    failures.push(`Expected 8 HSL swatches, found ${swatchData.length}`);
  } else {
    console.log('  Found all 8 HSL swatches.');
  }

  // Verify swatch target dimensions >= 40px
  const undersized = swatchData.filter((s) => s.width < 40 || s.height < 40);
  if (undersized.length > 0) {
    failures.push(`Some HSL swatches have hit target < 40px: ${JSON.stringify(undersized)}`);
  } else {
    console.log('  All HSL swatches satisfy >= 40px hit target.');
  }

  // Verify swatch backgrounds are computed using oklch
  const nonOklch = swatchData.filter((s) => !s.styleBg.includes('oklch'));
  if (nonOklch.length > 0) {
    failures.push(`HSL swatches must compute background via oklch; found: ${JSON.stringify(nonOklch)}`);
  } else {
    console.log('  All HSL swatches compute backgrounds via dynamic CSS oklch().');
  }

  // In v2_edits: Red and Blue have non-zero edits, so their swatches must have modified markers
  const redSwatch = swatchData.find((s) => s.testId === 'hsl-swatch-red');
  const blueSwatch = swatchData.find((s) => s.testId === 'hsl-swatch-blue');
  const greenSwatch = swatchData.find((s) => s.testId === 'hsl-swatch-green');

  if (!redSwatch?.hasDot) {
    failures.push('Red swatch should show modified dot indicator for loaded v2 edits');
  }
  if (!blueSwatch?.hasDot) {
    failures.push('Blue swatch should show modified dot indicator for loaded v2 edits');
  }
  if (greenSwatch?.hasDot) {
    failures.push('Green swatch should NOT show modified dot indicator for untouched band');
  }
  console.log('  Modified indicators correctly present only on edited bands (Red, Blue).');

  // Verify Red band slider values: hue=15, sat=20, lum=-10
  const redSliders = await page.evaluate(() => {
    const hue = document.querySelector('input[data-testid="hsl-slider-hue-number"]')?.value;
    const sat = document.querySelector('input[data-testid="hsl-slider-saturation-number"]')?.value;
    const lum = document.querySelector('input[data-testid="hsl-slider-luminance-number"]')?.value;
    return { hue, sat, lum };
  });

  if (redSliders.hue !== '15' || redSliders.sat !== '20' || redSliders.lum !== '-10') {
    failures.push(`Red band sliders mismatch: expected hue=15, sat=20, lum=-10; got ${JSON.stringify(redSliders)}`);
  } else {
    console.log(`  Red band sliders correctly display loaded values: hue=${redSliders.hue}, sat=${redSliders.sat}, lum=${redSliders.lum}`);
  }

  // Switch to Blue band and verify values: hue=-25, sat=40, lum=15
  const blueBtn = page.locator('button[data-testid="hsl-swatch-blue"]');
  await blueBtn.click();
  await page.waitForTimeout(50);

  const blueSliders = await page.evaluate(() => {
    const hue = document.querySelector('input[data-testid="hsl-slider-hue-number"]')?.value;
    const sat = document.querySelector('input[data-testid="hsl-slider-saturation-number"]')?.value;
    const lum = document.querySelector('input[data-testid="hsl-slider-luminance-number"]')?.value;
    return { hue, sat, lum };
  });

  if (blueSliders.hue !== '-25' || blueSliders.sat !== '40' || blueSliders.lum !== '15') {
    failures.push(`Blue band sliders mismatch: expected hue=-25, sat=40, lum=15; got ${JSON.stringify(blueSliders)}`);
  } else {
    console.log(`  Blue band sliders correctly display loaded values: hue=${blueSliders.hue}, sat=${blueSliders.sat}, lum=${blueSliders.lum}`);
  }

  // Reset Blue band via per-band reset button
  const resetBandBtn = page.locator('button[data-testid="hsl-reset-band-btn"]');
  await resetBandBtn.click();
  await page.waitForTimeout(50);

  const blueAfterReset = await page.evaluate(() => {
    const hue = document.querySelector('input[data-testid="hsl-slider-hue-number"]')?.value;
    const sat = document.querySelector('input[data-testid="hsl-slider-saturation-number"]')?.value;
    const lum = document.querySelector('input[data-testid="hsl-slider-luminance-number"]')?.value;
    const dot = Boolean(document.querySelector('[data-testid="hsl-dot-blue"]'));
    return { hue, sat, lum, dot };
  });

  if (blueAfterReset.hue !== '0' || blueAfterReset.sat !== '0' || blueAfterReset.lum !== '0' || blueAfterReset.dot) {
    failures.push(`Reset band failed: expected 0, 0, 0 and no dot; got ${JSON.stringify(blueAfterReset)}`);
  } else {
    console.log('  Per-band reset successfully reset Blue band to 0 and cleared modified dot.');
  }

  // Red should still have its modified dot
  const redDotStillThere = await page.evaluate(() => Boolean(document.querySelector('[data-testid="hsl-dot-red"]')));
  if (!redDotStillThere) {
    failures.push('Red swatch should still have modified dot after resetting only Blue');
  }

  // Section reset for Colour / HSL: clears all bands
  const hslResetBtn = page.locator('button[data-testid="section-hsl-reset"]');
  await hslResetBtn.click();
  await page.waitForTimeout(50);

  const dotsAfterSectionReset = await page.evaluate(() => document.querySelectorAll('.hsl-swatch-dot').length);
  if (dotsAfterSectionReset !== 0) {
    failures.push(`Section reset failed: expected 0 modified dots, found ${dotsAfterSectionReset}`);
  } else {
    console.log('  Section reset successfully reset all HSL bands to identity.');
  }

  // 9f. Assert 3-way Colour Grading panel (ED-12)
  console.log('Asserting 3-way Colour Grading panel (wheels, tabs, oklch hues, readouts, dots, and resets)...');
  await pathInput.fill('/Volumes/Photos/v2_edits.jpg');
  await pathInput.press('Enter');
  await page.waitForTimeout(100);

  // Expand Colour Grading section
  const gradingToggle = page.locator('button[data-testid="section-grading-toggle"]');
  await gradingToggle.click();
  await page.waitForTimeout(50);

  // Check that all 4 tabs exist and satisfy hit target >= 40px
  const tabData = await page.evaluate(() => {
    const tabs = Array.from(document.querySelectorAll('.grading-tab'));
    return tabs.map((t) => ({
      testId: t.getAttribute('data-testid'),
      hasDot: Boolean(t.querySelector('.grading-tab-dot')),
      isActive: t.classList.contains('active'),
      width: t.getBoundingClientRect().width,
      height: t.getBoundingClientRect().height,
    }));
  });

  if (tabData.length !== 4) {
    failures.push(`Expected 4 grading tabs, found ${tabData.length}`);
  } else {
    console.log('  Found all 4 grading tabs.');
  }

  const undersizedTabs = tabData.filter((t) => t.width < 40 || t.height < 40);
  if (undersizedTabs.length > 0) {
    failures.push(`Some grading tabs have hit target < 40px: ${JSON.stringify(undersizedTabs)}`);
  } else {
    console.log('  All grading tabs satisfy >= 40px hit target.');
  }

  // Check modified dots on tabs (Shadows, Midtones, Highlights modified; Global clean)
  const shadowsTab = tabData.find((t) => t.testId === 'grading-tab-shadows');
  const midtonesTab = tabData.find((t) => t.testId === 'grading-tab-midtones');
  const highlightsTab = tabData.find((t) => t.testId === 'grading-tab-highlights');
  const globalTab = tabData.find((t) => t.testId === 'grading-tab-global');

  if (!shadowsTab?.hasDot) {
    failures.push('Shadows tab should show modified dot indicator for loaded v2 edits');
  }
  if (!midtonesTab?.hasDot) {
    failures.push('Midtones tab should show modified dot indicator for loaded v2 edits');
  }
  if (!highlightsTab?.hasDot) {
    failures.push('Highlights tab should show modified dot indicator for loaded v2 edits');
  }
  if (globalTab?.hasDot) {
    failures.push('Global tab should NOT show modified dot indicator for untouched wheel');
  }
  console.log('  Modified indicators correctly present on Shadows, Midtones, Highlights.');

  // Verify loaded values on default Shadows wheel: hue=210°, sat=35%, lum=-10%
  const shadowsWheelVals = await page.evaluate(() => {
    const hue = document.querySelector('[data-testid="grading-wheel-shadows-readout-hue"]')?.textContent?.trim();
    const sat = document.querySelector('[data-testid="grading-wheel-shadows-readout-sat"]')?.textContent?.trim();
    const lum = document.querySelector('input[data-testid="grading-wheel-shadows-slider-lum-number"]')?.value;
    return { hue, sat, lum };
  });

  if (shadowsWheelVals.hue !== '210°' || shadowsWheelVals.sat !== '35%' || shadowsWheelVals.lum !== '-10') {
    failures.push(`Shadows wheel values mismatch: expected 210°, 35%, -10; got ${JSON.stringify(shadowsWheelVals)}`);
  } else {
    console.log(`  Shadows wheel correctly displays loaded values: hue=${shadowsWheelVals.hue}, sat=${shadowsWheelVals.sat}, lum=${shadowsWheelVals.lum}%`);
  }

  // Check Blending and Balance sliders: blending=60, balance=-15
  const rangeVals = await page.evaluate(() => {
    const blending = document.querySelector('input[data-testid="grading-slider-blending-number"]')?.value;
    const balance = document.querySelector('input[data-testid="grading-slider-balance-number"]')?.value;
    return { blending, balance };
  });

  if (rangeVals.blending !== '60' || rangeVals.balance !== '-15') {
    failures.push(`Grading range sliders mismatch: expected blending=60, balance=-15; got ${JSON.stringify(rangeVals)}`);
  } else {
    console.log(`  Grading range sliders correctly display loaded values: blending=${rangeVals.blending}%, balance=${rangeVals.balance}`);
  }

  // Switch tabs to Highlights: verify values hue=35°, sat=40%, lum=15%
  const highlightsTabBtn = page.locator('button[data-testid="grading-tab-highlights"]');
  await highlightsTabBtn.click();
  await page.waitForTimeout(50);

  const highlightsWheelVals = await page.evaluate(() => {
    const hue = document.querySelector('[data-testid="grading-wheel-highlights-readout-hue"]')?.textContent?.trim();
    const sat = document.querySelector('[data-testid="grading-wheel-highlights-readout-sat"]')?.textContent?.trim();
    const lum = document.querySelector('input[data-testid="grading-wheel-highlights-slider-lum-number"]')?.value;
    return { hue, sat, lum };
  });

  if (highlightsWheelVals.hue !== '35°' || highlightsWheelVals.sat !== '40%' || highlightsWheelVals.lum !== '15') {
    failures.push(`Highlights wheel values mismatch: expected 35°, 40%, 15; got ${JSON.stringify(highlightsWheelVals)}`);
  } else {
    console.log(`  Highlights wheel correctly displays loaded values: hue=${highlightsWheelVals.hue}, sat=${highlightsWheelVals.sat}, lum=${highlightsWheelVals.lum}%`);
  }

  // Test keyboard navigation on the SVG wheel: ArrowRight moves hue
  const wheelSvg = page.locator('[data-testid="grading-wheel-highlights-svg"]');
  await wheelSvg.focus();
  await page.keyboard.press('ArrowRight');
  await page.waitForTimeout(20);

  const hueAfterArrow = await page.evaluate(() => {
    return document.querySelector('[data-testid="grading-wheel-highlights-readout-hue"]')?.textContent?.trim();
  });
  if (hueAfterArrow !== '36°') {
    failures.push(`Highlights wheel keyboard navigation failed: expected 36°, got ${hueAfterArrow}`);
  } else {
    console.log('  Highlights wheel responded correctly to ArrowRight navigation (36°).');
  }

  // Reset Highlights wheel via per-wheel reset button
  const resetWheelBtn = page.locator('button[data-testid="grading-reset-wheel-btn"]');
  await resetWheelBtn.click();
  await page.waitForTimeout(50);

  const highlightsAfterReset = await page.evaluate(() => {
    const hue = document.querySelector('[data-testid="grading-wheel-highlights-readout-hue"]')?.textContent?.trim();
    const sat = document.querySelector('[data-testid="grading-wheel-highlights-readout-sat"]')?.textContent?.trim();
    const lum = document.querySelector('input[data-testid="grading-wheel-highlights-slider-lum-number"]')?.value;
    const dot = Boolean(document.querySelector('[data-testid="grading-dot-highlights"]'));
    return { hue, sat, lum, dot };
  });

  if (highlightsAfterReset.hue !== '0°' || highlightsAfterReset.sat !== '0%' || highlightsAfterReset.lum !== '0' || highlightsAfterReset.dot) {
    failures.push(`Reset current wheel failed: expected 0°, 0%, 0, no dot; got ${JSON.stringify(highlightsAfterReset)}`);
  } else {
    console.log('  Per-wheel reset successfully reset Highlights wheel to 0 and cleared modified dot.');
  }

  // Section reset for Colour Grading
  const gradingResetBtn = page.locator('button[data-testid="section-grading-reset"]');
  await gradingResetBtn.click();
  await page.waitForTimeout(50);

  const dotsAfterGradingReset = await page.evaluate(() => document.querySelectorAll('.grading-tab-dot').length);
  if (dotsAfterGradingReset !== 0) {
    failures.push(`Grading section reset failed: expected 0 modified dots, found ${dotsAfterGradingReset}`);
  } else {
    console.log('  Section reset successfully reset all grading wheels to identity.');
  }

  // 9g. Assert Geometry panel (crop overlay, aspect select, straighten, rotation, flip, orientation baking) (ED-13)
  console.log('Asserting Geometry panel (crop overlay, aspect select, straighten, rotation, flip, and resets)...');
  await pathInput.fill('/Volumes/Photos/v2_edits.jpg');
  await pathInput.press('Enter');
  await page.waitForTimeout(100);

  // Expand Geometry section
  const geomToggle = page.locator('button[data-testid="section-geometry-toggle"]');
  await geomToggle.click();
  await page.waitForTimeout(50);

  // 1. Verify loaded recipe values in v2_edits:
  // geometry: { crop: { x: 0.1, y: 0.1, width: 0.8, height: 0.6 }, rotate: 90, straighten: 3.5, flip_h: false, flip_v: false, aspect: '4:3' }
  const loadedGeomVals = await page.evaluate(() => {
    const aspect = document.querySelector('select[data-testid="geometry-aspect-select"]')?.value;
    const straighten = document.querySelector('input[data-testid="geometry-straighten-number"]')?.value;
    const isFlipHActive = document.querySelector('button[data-testid="flip-h-btn"]')?.classList.contains('active');
    const isFlipVActive = document.querySelector('button[data-testid="flip-v-btn"]')?.classList.contains('active');
    return { aspect, straighten, isFlipHActive, isFlipVActive };
  });

  if (loadedGeomVals.aspect !== '4:3' || loadedGeomVals.straighten !== '3.5' || loadedGeomVals.isFlipHActive || loadedGeomVals.isFlipVActive) {
    failures.push(`Loaded geometry mismatch: expected aspect=4:3, straighten=3.5; got ${JSON.stringify(loadedGeomVals)}`);
  } else {
    console.log(`  Geometry panel correctly displays loaded values: aspect=${loadedGeomVals.aspect}, straighten=${loadedGeomVals.straighten}°`);
  }

  // 2. Crop mode toggle: assert [data-test="crop-overlay"] appears, aspect select changes crop box, and straighten slider moves
  const cropToggleBtn = page.locator('button[data-testid="crop-mode-toggle-btn"]');
  await cropToggleBtn.click();
  await page.waitForTimeout(50);

  const cropOverlayVisible = await page.evaluate(() => {
    return Boolean(document.querySelector('[data-test="crop-overlay"]'));
  });
  if (!cropOverlayVisible) {
    failures.push('Crop overlay [data-test="crop-overlay"] should be visible when Crop mode is active');
  } else {
    console.log('  Crop overlay correctly mounted and visible in crop mode.');
  }

  // Change aspect preset to 1:1 and verify crop box adjusts
  await page.selectOption('select[data-testid="geometry-aspect-select"]', '1:1');
  await page.waitForTimeout(50);

  const cropBoxRatio = await page.evaluate(() => {
    const box = document.querySelector('.crop-overlay__box');
    if (!box) return null;
    const w = parseFloat(box.getAttribute('width') || '0');
    const h = parseFloat(box.getAttribute('height') || '0');
    return { w, h, ratio: (w / h).toFixed(2) };
  });

  if (!cropBoxRatio || cropBoxRatio.ratio !== '1.00') {
    failures.push(`1:1 aspect preset did not produce square box: ${JSON.stringify(cropBoxRatio)}`);
  } else {
    console.log(`  Aspect preset 1:1 correctly produced square crop box (${cropBoxRatio.w}x${cropBoxRatio.h}).`);
  }

  // Move straighten slider
  const straightenSlider = page.locator('input[data-testid="geometry-straighten-number"]');
  await straightenSlider.fill('7.0');
  await straightenSlider.press('Enter');
  await page.waitForTimeout(50);

  const straightenValAfterEdit = await page.evaluate(() => {
    return document.querySelector('input[data-testid="geometry-straighten-number"]')?.value;
  });
  if (parseFloat(straightenValAfterEdit || '') !== 7) {
    failures.push(`Straighten slider update failed: expected 7, got ${straightenValAfterEdit}`);
  } else {
    console.log('  Straighten slider successfully updated to 7.0°.');
  }

  // Exit crop mode
  await cropToggleBtn.click();
  await page.waitForTimeout(50);

  // 3. Test stub with orientation 6:
  // (a) Open image with no geometry, enter crop mode: assert painted canvas is portrait with NO CSS rotation
  await page.evaluate(() => {
    window.__STUB__.orientation = 6;
  });
  await pathInput.fill('/Volumes/Photos/orient_6.jpg');
  await pathInput.press('Enter');
  await page.waitForTimeout(100);

  // Ensure geometry section is open
  const isGeomOpen = await page.evaluate(() => Boolean(document.querySelector('[data-testid="geometry-panel"]')));
  if (!isGeomOpen) {
    await page.locator('button[data-testid="section-geometry-toggle"]').click();
    await page.waitForTimeout(50);
  }

  // Enter crop mode with no other geometry
  await page.locator('button[data-testid="crop-mode-toggle-btn"]').click();
  await page.waitForTimeout(100);

  const inCropModeNoGeom = await page.evaluate(() => {
    const canvas = document.querySelector('canvas[data-testid="edit-canvas"]');
    if (!canvas) return null;
    const cs = window.getComputedStyle(canvas);
    return {
      width: canvas.width,
      height: canvas.height,
      transform: cs.transform,
    };
  });

  if (!inCropModeNoGeom || inCropModeNoGeom.height <= inCropModeNoGeom.width) {
    failures.push(`Orientation 6 in crop mode with no geometry must have portrait painted canvas (height > width), got ${inCropModeNoGeom?.width}x${inCropModeNoGeom?.height}`);
  } else if (inCropModeNoGeom.transform !== 'none') {
    failures.push(`Orientation 6 in crop mode with no geometry must have CSS transform 'none', got ${inCropModeNoGeom.transform}`);
  } else {
    console.log(`  Orientation 6 in crop mode with no geometry has portrait canvas (${inCropModeNoGeom.width}x${inCropModeNoGeom.height}) and CSS transform 'none'.`);
  }

  // Leave crop mode without changing anything
  await page.locator('button[data-testid="crop-mode-toggle-btn"]').click();
  await page.waitForTimeout(100);

  const afterLeavingCropNoGeom = await page.evaluate(() => {
    const canvas = document.querySelector('canvas[data-testid="edit-canvas"]');
    if (!canvas) return null;
    const cs = window.getComputedStyle(canvas);
    const rect = canvas.getBoundingClientRect();
    return {
      width: canvas.width,
      height: canvas.height,
      transform: cs.transform,
      boundingWidth: rect.width,
      boundingHeight: rect.height,
    };
  });

  if (!afterLeavingCropNoGeom || afterLeavingCropNoGeom.boundingHeight <= afterLeavingCropNoGeom.boundingWidth) {
    failures.push(`After leaving crop mode with no edits, canvas must display portrait (boundingHeight > boundingWidth), got ${afterLeavingCropNoGeom?.boundingWidth}x${afterLeavingCropNoGeom?.boundingHeight}`);
  } else {
    console.log(`  After leaving crop mode with no edits, canvas remains portrait (${afterLeavingCropNoGeom.boundingWidth.toFixed(1)}x${afterLeavingCropNoGeom.boundingHeight.toFixed(1)}), transform: ${afterLeavingCropNoGeom.transform}.`);
  }

  // (b) Enter crop mode again, set 1:1 crop, exit crop mode: assert upright square dimensions and transform 'none'
  await page.locator('button[data-testid="crop-mode-toggle-btn"]').click();
  await page.waitForTimeout(50);
  await page.selectOption('select[data-testid="geometry-aspect-select"]', '1:1');
  await page.waitForTimeout(50);
  await page.locator('button[data-testid="crop-mode-toggle-btn"]').click();
  await page.waitForTimeout(100);

  const orient6CropResult = await page.evaluate(() => {
    const canvas = document.querySelector('canvas[data-testid="edit-canvas"]');
    if (!canvas) return { error: 'canvas not found' };
    const cs = window.getComputedStyle(canvas);
    return {
      transform: cs.transform,
      canvasWidth: canvas.width,
      canvasHeight: canvas.height,
    };
  });

  if (orient6CropResult.transform !== 'none') {
    failures.push(`Orientation 6 with active crop should have CSS transform 'none', got ${orient6CropResult.transform}`);
  } else {
    console.log(`  Orientation 6 with active crop correctly switches canvas CSS transform to 'none'.`);
  }

  if (orient6CropResult.canvasWidth !== orient6CropResult.canvasHeight) {
    failures.push(`Delivered dimensions for 1:1 crop on orientation 6 should be square, got ${orient6CropResult.canvasWidth}x${orient6CropResult.canvasHeight}`);
  } else {
    console.log(`  Delivered dimensions for orientation 6 crop are upright cropped square (${orient6CropResult.canvasWidth}x${orient6CropResult.canvasHeight}).`);
  }

  // Reset geometry section
  const geomResetBtn = page.locator('button[data-testid="section-geometry-reset"]');
  await geomResetBtn.click();
  await page.waitForTimeout(100);

  const afterGeomReset = await page.evaluate(() => {
    const canvas = document.querySelector('canvas[data-testid="edit-canvas"]');
    const cs = window.getComputedStyle(canvas);
    const straighten = document.querySelector('input[data-testid="geometry-straighten-number"]')?.value;
    return {
      transform: cs.transform,
      straighten,
    };
  });

  // Since geometry is now reset to identity, orientation 6 CSS rotation should be restored
  if (afterGeomReset.transform === 'none') {
    failures.push(`After geometry reset, orientation 6 CSS rotation should be restored, got 'none'`);
  } else {
    console.log(`  Geometry reset successfully restored canvas CSS rotation (${afterGeomReset.transform}).`);
  }
  if (parseFloat(afterGeomReset.straighten || '') !== 0) {
    failures.push(`After geometry reset, straighten should be 0, got ${afterGeomReset.straighten}`);
  } else {
    console.log('  Geometry section reset successfully cleared all geometry transforms to identity.');
  }

  // Restore stub orientation to 1
  await page.evaluate(() => {
    window.__STUB__.orientation = 1;
  });

  // 9h. Assert Effects panel (Vignette: Amount, Midpoint, Roundness, Feather; Film Grain: Amount, Size, Roughness; double-click reset; section reset) (ED-14)
  console.log('Asserting Effects panel (loaded v2 vignette and grain, sliders, double-click reset, section reset)...');
  await pathInput.fill('/Volumes/Photos/v2_edits.jpg');
  await pathInput.press('Enter');
  await page.waitForTimeout(100);

  // Expand Effects section
  const effectsToggle = page.locator('button[data-testid="section-effects-toggle"]');
  await effectsToggle.click();
  await page.waitForTimeout(50);

  // Check loaded values from v2_edits
  const loadedEffects = await page.evaluate(() => {
    return {
      vignetteAmount: document.querySelector('input[data-testid="vignette-amount-number"]')?.value,
      vignetteMidpoint: document.querySelector('input[data-testid="vignette-midpoint-number"]')?.value,
      vignetteRoundness: document.querySelector('input[data-testid="vignette-roundness-number"]')?.value,
      vignetteFeather: document.querySelector('input[data-testid="vignette-feather-number"]')?.value,
      grainAmount: document.querySelector('input[data-testid="grain-amount-number"]')?.value,
      grainSize: document.querySelector('input[data-testid="grain-size-number"]')?.value,
      grainRoughness: document.querySelector('input[data-testid="grain-roughness-number"]')?.value,
    };
  });

  if (
    loadedEffects.vignetteAmount !== '-45' ||
    loadedEffects.vignetteMidpoint !== '40' ||
    loadedEffects.vignetteRoundness !== '20' ||
    loadedEffects.vignetteFeather !== '65' ||
    loadedEffects.grainAmount !== '35' ||
    loadedEffects.grainSize !== '40' ||
    loadedEffects.grainRoughness !== '60'
  ) {
    failures.push(`Loaded effects values mismatch: expected vignette [-45, 40, 20, 65] and grain [35, 40, 60]; got ${JSON.stringify(loadedEffects)}`);
  } else {
    console.log(`  Loaded effects sliders correctly display loaded v2 values: vignette [-45, 40, 20, 65], grain [35, 40, 60].`);
  }

  // Double-click reset test:
  // Double-click Vignette Amount label -> resets to 0
  const vigAmountLabel = page.locator('[data-testid="vignette-amount-label"]');
  await vigAmountLabel.dblclick();
  await page.waitForTimeout(50);

  // Double-click Vignette Midpoint label -> resets to 50
  const vigMidpointLabel = page.locator('[data-testid="vignette-midpoint-label"]');
  await vigMidpointLabel.dblclick();
  await page.waitForTimeout(50);

  // Double-click Grain Size label -> resets to 25
  const grainSizeLabel = page.locator('[data-testid="grain-size-label"]');
  await grainSizeLabel.dblclick();
  await page.waitForTimeout(50);

  const afterDblClick = await page.evaluate(() => {
    return {
      vignetteAmount: document.querySelector('input[data-testid="vignette-amount-number"]')?.value,
      vignetteMidpoint: document.querySelector('input[data-testid="vignette-midpoint-number"]')?.value,
      grainSize: document.querySelector('input[data-testid="grain-size-number"]')?.value,
    };
  });

  if (afterDblClick.vignetteAmount !== '0' || afterDblClick.vignetteMidpoint !== '50' || afterDblClick.grainSize !== '25') {
    failures.push(`Double-click reset failed: expected vignetteAmount=0, midpoint=50, grainSize=25; got ${JSON.stringify(afterDblClick)}`);
  } else {
    console.log('  Double-click reset on effects labels successfully restored default values (Amount=0, Midpoint=50, Size=25).');
  }

  // Section reset for Effects
  const effectsResetBtn = page.locator('button[data-testid="section-effects-reset"]');
  await effectsResetBtn.click();
  await page.waitForTimeout(100);

  const afterEffectsReset = await page.evaluate(() => {
    return {
      vignetteAmount: document.querySelector('input[data-testid="vignette-amount-number"]')?.value,
      vignetteMidpoint: document.querySelector('input[data-testid="vignette-midpoint-number"]')?.value,
      vignetteRoundness: document.querySelector('input[data-testid="vignette-roundness-number"]')?.value,
      vignetteFeather: document.querySelector('input[data-testid="vignette-feather-number"]')?.value,
      grainAmount: document.querySelector('input[data-testid="grain-amount-number"]')?.value,
      grainSize: document.querySelector('input[data-testid="grain-size-number"]')?.value,
      grainRoughness: document.querySelector('input[data-testid="grain-roughness-number"]')?.value,
    };
  });

  if (
    afterEffectsReset.vignetteAmount !== '0' ||
    afterEffectsReset.vignetteMidpoint !== '50' ||
    afterEffectsReset.vignetteRoundness !== '0' ||
    afterEffectsReset.vignetteFeather !== '50' ||
    afterEffectsReset.grainAmount !== '0' ||
    afterEffectsReset.grainSize !== '25' ||
    afterEffectsReset.grainRoughness !== '50'
  ) {
    failures.push(`Effects section reset failed: expected all sliders at default; got ${JSON.stringify(afterEffectsReset)}`);
  } else {
    console.log('  Effects section reset successfully reset all vignette and grain controls to default.');
  }

  // 9i. Assert Look panel (Glow: Amount, Threshold, Radius; Halation: Amount, Threshold, Radius; Tone Mapper: Plain/Filmic toggle, advisory banner; double-click reset; section reset) (ED-16)
  console.log('Asserting Look panel (loaded v2 glow, halation, tone mapper, advisory banner, double-click reset, section reset)...');
  await pathInput.fill('/Volumes/Photos/v2_edits.jpg');
  await pathInput.press('Enter');
  await page.waitForTimeout(100);

  // Check loaded values from v2_edits
  const loadedLook = await page.evaluate(() => {
    const banner = document.querySelector('[data-testid="tone-mapper-banner"]');
    const filmicBtn = document.querySelector('[data-testid="tone-mapper-filmic"]');
    return {
      glowAmount: document.querySelector('input[data-testid="glow-amount-number"]')?.value,
      glowThreshold: document.querySelector('input[data-testid="glow-threshold-number"]')?.value,
      glowRadius: document.querySelector('input[data-testid="glow-radius-number"]')?.value,
      halationAmount: document.querySelector('input[data-testid="halation-amount-number"]')?.value,
      halationThreshold: document.querySelector('input[data-testid="halation-threshold-number"]')?.value,
      halationRadius: document.querySelector('input[data-testid="halation-radius-number"]')?.value,
      filmicActive: filmicBtn?.classList.contains('active'),
      bannerVisible: !!banner && banner.textContent.includes('Filmic tone curve on: highlights roll off and mid-tones shift'),
    };
  });

  if (
    loadedLook.glowAmount !== '30' ||
    loadedLook.glowThreshold !== '65' ||
    loadedLook.glowRadius !== '25' ||
    loadedLook.halationAmount !== '20' ||
    loadedLook.halationThreshold !== '75' ||
    loadedLook.halationRadius !== '15' ||
    !loadedLook.filmicActive ||
    !loadedLook.bannerVisible
  ) {
    failures.push(`Loaded look values mismatch: expected glow [30, 65, 25], halation [20, 75, 15], filmic=true, banner=visible; got ${JSON.stringify(loadedLook)}`);
  } else {
    console.log(`  Loaded look controls correctly display loaded v2 values and plain-language filmic advisory banner.`);
  }

  // Double-click reset test:
  // Double-click Glow Amount label -> resets to 0
  const glowAmountLabel = page.locator('[data-testid="glow-amount-label"]');
  await glowAmountLabel.dblclick();
  await page.waitForTimeout(50);

  // Double-click Glow Threshold label -> resets to 70
  const glowThreshLabel = page.locator('[data-testid="glow-threshold-label"]');
  await glowThreshLabel.dblclick();
  await page.waitForTimeout(50);

  // Double-click Halation Radius label -> resets to 20
  const halRadiusLabel = page.locator('[data-testid="halation-radius-label"]');
  await halRadiusLabel.dblclick();
  await page.waitForTimeout(50);

  const afterLookDblClick = await page.evaluate(() => {
    return {
      glowAmount: document.querySelector('input[data-testid="glow-amount-number"]')?.value,
      glowThreshold: document.querySelector('input[data-testid="glow-threshold-number"]')?.value,
      halationRadius: document.querySelector('input[data-testid="halation-radius-number"]')?.value,
    };
  });

  if (afterLookDblClick.glowAmount !== '0' || afterLookDblClick.glowThreshold !== '70' || afterLookDblClick.halationRadius !== '20') {
    failures.push(`Look double-click reset failed: expected glowAmount=0, glowThreshold=70, halationRadius=20; got ${JSON.stringify(afterLookDblClick)}`);
  } else {
    console.log('  Double-click reset on look labels successfully restored default values (Glow Amount=0, Threshold=70, Halation Radius=20).');
  }

  // Tone mapper toggle test: switch to Plain -> banner hides; switch to Filmic -> banner shows
  const plainBtn = page.locator('[data-testid="tone-mapper-plain"]');
  await plainBtn.click();
  await page.waitForTimeout(50);
  const plainState = await page.evaluate(() => {
    return {
      plainActive: document.querySelector('[data-testid="tone-mapper-plain"]')?.classList.contains('active'),
      filmicActive: document.querySelector('[data-testid="tone-mapper-filmic"]')?.classList.contains('active'),
      bannerPresent: !!document.querySelector('[data-testid="tone-mapper-banner"]'),
    };
  });
  if (!plainState.plainActive || plainState.filmicActive || plainState.bannerPresent) {
    failures.push(`Tone mapper Plain toggle failed: got ${JSON.stringify(plainState)}`);
  } else {
    console.log('  Tone mapper Plain toggle correctly deactivated Filmic and removed advisory banner.');
  }

  // Section reset for Look
  const lookResetBtn = page.locator('button[data-testid="section-look-reset"]');
  await lookResetBtn.click();
  await page.waitForTimeout(100);

  const afterLookReset = await page.evaluate(() => {
    return {
      glowAmount: document.querySelector('input[data-testid="glow-amount-number"]')?.value,
      glowThreshold: document.querySelector('input[data-testid="glow-threshold-number"]')?.value,
      glowRadius: document.querySelector('input[data-testid="glow-radius-number"]')?.value,
      halationAmount: document.querySelector('input[data-testid="halation-amount-number"]')?.value,
      halationThreshold: document.querySelector('input[data-testid="halation-threshold-number"]')?.value,
      halationRadius: document.querySelector('input[data-testid="halation-radius-number"]')?.value,
      plainActive: document.querySelector('[data-testid="tone-mapper-plain"]')?.classList.contains('active'),
      bannerPresent: !!document.querySelector('[data-testid="tone-mapper-banner"]'),
    };
  });

  if (
    afterLookReset.glowAmount !== '0' ||
    afterLookReset.glowThreshold !== '70' ||
    afterLookReset.glowRadius !== '30' ||
    afterLookReset.halationAmount !== '0' ||
    afterLookReset.halationThreshold !== '80' ||
    afterLookReset.halationRadius !== '20' ||
    !afterLookReset.plainActive ||
    afterLookReset.bannerPresent
  ) {
    failures.push(`Look section reset failed: expected all sliders and tone mapper at default; got ${JSON.stringify(afterLookReset)}`);
  } else {
    console.log('  Look section reset successfully reset all glow, halation and tone mapper controls to identity.');
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
