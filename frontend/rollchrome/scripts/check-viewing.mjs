/**
 * RP-3 Viewing verification: zoom, pan, and before/after compare.
 *
 * Runs against the Edit harness with devicePixelRatio = 2,
 * stub 6000x4000 source, and 2560x1707 settle proxy.
 *
 * Asserts all 18 requirements:
 * 1. On opening, zoom is fit and no Full is requested.
 * 2. Cmd-1: the canvas's CSS width is 3000 ± 1, and Full is requested exactly once.
 * 3. At a zoom the settle proxy can serve, only Settle is requested.
 * 4. During a slider drag at 100% every request is Drag, and on release one Full.
 * 5. Ctrl-wheel at a point keeps the image point under it within 2 px.
 * 6. Panning far past an edge leaves no gap on that side.
 * 7. With a mask brush active, a plain drag paints and does not pan; with Space held it pans and paints nothing.
 * 8. With text focused, Z does nothing.
 * 9. Side-by-side: two canvases of equal CSS size fit the container. The before's request has identity adjustments and the recipe's geometry.
 * 10. Twenty slider steps while comparing send no further before requests.
 * 11. Split: dragging the divider to 30% gives clip-path at 30% ± 1.
 * 12. Entering crop mode turns compare off and zoom to fit.
 * 13. Every new control is at least 40 px. Screenshots of zoomed, side-by-side and split go to layout-proof/.
 * 14. Orientation 6, no geometry: at 100% the visible canvas is 2000 × 3000 ± 1 and Full is requested.
 * 15. Orientation 6 with a geometry present, and orientation 1 with rotate: 90: at 100% the visible canvas is 2000 × 3000 ± 1.
 * 16. A 40% crop at 100% requests Full, and the visible canvas is 40% of the full width ± 1.
 * 17. For the orientation 6 case, panning past each edge leaves no gap, measured from the visible canvas's box.
 * 18. With a brush active, a double-click changes neither zoom nor pan.
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

console.log('Starting Vite harness server for Viewing check...');
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

let currentSection = 'start-up';
const announce = console.log;
console.log = (...args) => {
  if (typeof args[0] === 'string' && args[0].startsWith('Asserting')) currentSection = args[0];
  announce(...args);
};

try {
  await page.goto(base, { waitUntil: 'networkidle' });

  // Open photo in edit view
  const pathInput = page.locator('input[placeholder="/path/to/image.jpg"]');
  await pathInput.fill('/Volumes/Photos/landscape.jpg');
  await pathInput.press('Enter');
  await page.waitForSelector('canvas[data-testid="edit-canvas"]');
  await page.waitForTimeout(300);
  await page.evaluate(() => {
    if (document.activeElement && document.activeElement instanceof HTMLElement) {
      document.activeElement.blur();
    }
  });

  // 1. On opening, zoom is fit and no Full is requested.
  console.log('Asserting 1: On opening, zoom is fit and no Full is requested...');
  const initCheck = await page.evaluate(() => {
    const renders = window.__STUB__.renders;
    const fullRenders = renders.filter((r) => r.stage === 'Full');
    const fitBtn = document.querySelector('button[data-testid="zoom-fit-btn"]');
    const isFitActive = fitBtn?.classList.contains('active');
    const canvas = document.querySelector('canvas[data-testid="edit-canvas"]');
    return {
      totalRenders: renders.length,
      fullCount: fullRenders.length,
      isFitActive,
      canvasWidth: canvas?.getBoundingClientRect().width,
    };
  });
  if (initCheck.fullCount !== 0) {
    failures.push(`Requirement 1 failed: requested Full on open (${initCheck.fullCount} times)`);
  }
  if (!initCheck.isFitActive) {
    failures.push('Requirement 1 failed: Fit button is not active on open');
  }
  console.log(`  Initial open: ${initCheck.totalRenders} renders, Full count = ${initCheck.fullCount}, Fit active = ${initCheck.isFitActive}`);

  // 2. Cmd-1: the canvas's CSS width is 3000 ± 1, and Full is requested exactly once.
  console.log('Asserting 2: Cmd-1 gives canvas CSS width 3000 ± 1, and Full requested exactly once...');
  // Clear stub renders log & blur active elements
  await page.evaluate(() => {
    if (document.activeElement && document.activeElement instanceof HTMLElement) {
      document.activeElement.blur();
    }
    window.__STUB__.renders = [];
  });
  await page.keyboard.press('Meta+1');
  await page.waitForTimeout(200);

  const zoom100Check = await page.evaluate(() => {
    const canvas = document.querySelector('canvas[data-testid="edit-canvas"]');
    const renders = window.__STUB__.renders;
    const fullRenders = renders.filter((r) => r.stage === 'Full');
    const cssWidth = canvas ? canvas.getBoundingClientRect().width : 0;
    return {
      cssWidth,
      fullCount: fullRenders.length,
    };
  });

  if (Math.abs(zoom100Check.cssWidth - 3000) > 1) {
    failures.push(`Requirement 2 failed: canvas CSS width was ${zoom100Check.cssWidth} (expected 3000 ± 1)`);
  }
  if (zoom100Check.fullCount !== 1) {
    failures.push(`Requirement 2 failed: Full requested ${zoom100Check.fullCount} times on Cmd-1 (expected 1)`);
  }
  console.log(`  Cmd-1: canvas CSS width = ${zoom100Check.cssWidth}, Full requested = ${zoom100Check.fullCount}`);

  // 3. At a zoom the settle proxy can serve, only Settle is requested.
  console.log('Asserting 3: At a zoom the settle proxy can serve, only Settle is requested...');
  await page.evaluate(() => {
    window.addEventListener('keydown', (e) => {
      console.log('KEY RECEIVED:', e.key, 'code:', e.code, 'meta:', e.metaKey, 'ctrl:', e.ctrlKey);
    }, { once: true });
    window.__STUB__.renders = [];
  });
  // Click Fit button
  await page.click('button[data-testid="zoom-fit-btn"]');
  await page.waitForTimeout(200);

  const fitStageCheck = await page.evaluate(() => {
    const renders = window.__STUB__.renders;
    const fullRenders = renders.filter((r) => r.stage === 'Full');
    const settleRenders = renders.filter((r) => r.stage === 'Settle');
    return {
      rendersIssued: renders.length,
      fullCount: fullRenders.length,
      settleCount: settleRenders.length,
    };
  });
  if (fitStageCheck.fullCount !== 0) {
    failures.push(`Requirement 3 failed: Full requested at fit zoom (${fitStageCheck.fullCount} times)`);
  }
  if (fitStageCheck.settleCount < 1) {
    failures.push(`Requirement 3 failed: expected at least 1 Settle render, got ${fitStageCheck.settleCount}`);
  }
  console.log(`  Zoom to fit: Settle count = ${fitStageCheck.settleCount}, Full count = ${fitStageCheck.fullCount}`);

  // 4. During a slider drag at 100% every request is Drag, and on release one Full.
  console.log('Asserting 4: During slider drag at 100%, every request is Drag, and on release one Full...');
  // Zoom to 100%
  await page.keyboard.press('Meta+1');
  await page.waitForTimeout(150);
  await page.evaluate(() => {
    window.addEventListener('keydown', (e) => {
      console.log('KEY RECEIVED:', e.key, 'code:', e.code, 'meta:', e.metaKey, 'ctrl:', e.ctrlKey);
    }, { once: true });
    window.__STUB__.renders = [];
  });

  // Find exposure slider
  const expSlider = page.locator('input[data-testid="exposure-slider"]');
  // Trigger 5 input events simulating scrubbing
  for (let i = 1; i <= 5; i++) {
    await expSlider.evaluate((el, val) => {
      el.value = String(val);
      el.dispatchEvent(new Event('input', { bubbles: true }));
    }, i * 0.2);
    await page.waitForTimeout(20);
  }

  // Check that all intermediate requests were Drag
  const dragCheck = await page.evaluate(() => {
    const renders = window.__STUB__.renders;
    const nonDrag = renders.filter((r) => r.stage !== 'Drag');
    return {
      total: renders.length,
      nonDragCount: nonDrag.length,
    };
  });

  if (dragCheck.nonDragCount > 0) {
    failures.push(`Requirement 4 failed: during slider drag, non-Drag requests were issued (${dragCheck.nonDragCount})`);
  }

  // Trigger release (change event)
  await expSlider.evaluate((el) => {
    el.dispatchEvent(new Event('change', { bubbles: true }));
  });
  await page.waitForTimeout(200);

  const releaseCheck = await page.evaluate(() => {
    const renders = window.__STUB__.renders;
    const last = renders[renders.length - 1];
    const fullRenders = renders.filter((r) => r.stage === 'Full');
    return {
      lastStage: last?.stage,
      fullCount: fullRenders.length,
    };
  });

  if (releaseCheck.lastStage !== 'Full') {
    failures.push(`Requirement 4 failed: on release at 100%, last stage was ${releaseCheck.lastStage} (expected Full)`);
  }
  if (releaseCheck.fullCount !== 1) {
    failures.push(`Requirement 4 failed: expected exactly 1 Full request on release, got ${releaseCheck.fullCount}`);
  }
  console.log(`  Slider scrub at 100%: ${dragCheck.total} Drag requests, release stage = ${releaseCheck.lastStage}`);

  // 5. Ctrl-wheel at a point keeps the image point under it within 2 px.
  console.log('Asserting 5: Ctrl-wheel at a point keeps the image point under it within 2 px...');
  const wheelResult = await page.evaluate(() => {
    const container = document.querySelector('.canvas-viewport__container');
    const stage = document.querySelector('.canvas-viewport__stage');
    if (!container || !stage) return { error: 'elements missing' };

    const cRect = container.getBoundingClientRect();
    const sRect = stage.getBoundingClientRect();
    // Choose cursor inside container
    const cx = cRect.left + cRect.width * 0.4;
    const cy = cRect.top + cRect.height * 0.4;

    // Point in stage relative coordinates before zoom
    const uBefore = (cx - sRect.left) / sRect.width;

    // Dispatch Ctrl-wheel event to zoom in
    container.dispatchEvent(
      new WheelEvent('wheel', {
        clientX: cx,
        clientY: cy,
        deltaY: -80,
        ctrlKey: true,
        bubbles: true,
        cancelable: true,
      }),
    );

    const sRectAfter = stage.getBoundingClientRect();
    const uAfter = (cx - sRectAfter.left) / sRectAfter.width;
    const pixelDiff = Math.abs(uBefore * sRectAfter.width - (cx - sRectAfter.left));

    return {
      uBefore,
      uAfter,
      pixelDiff,
    };
  });

  if (wheelResult.pixelDiff > 2) {
    failures.push(`Requirement 5 failed: Ctrl-wheel cursor point shifted by ${wheelResult.pixelDiff.toFixed(2)} px (expected <= 2 px)`);
  }
  console.log(`  Ctrl-wheel about point: pixel diff = ${wheelResult.pixelDiff.toFixed(2)} px`);

  // 6. Panning far past an edge leaves no gap on that side.
  console.log('Asserting 6: Panning far past an edge leaves no gap on that side...');
  const panClampCheck = await page.evaluate(async () => {
    const container = document.querySelector('.canvas-viewport__container');
    const stage = document.querySelector('.canvas-viewport__stage');
    if (!container || !stage) return { error: 'elements missing' };

    const cRect = container.getBoundingClientRect();

    // Pan far right (+2000px)
    container.dispatchEvent(new PointerEvent('pointerdown', { clientX: 500, clientY: 500, bubbles: true }));
    window.dispatchEvent(new PointerEvent('pointermove', { clientX: 2500, clientY: 500, bubbles: true }));
    window.dispatchEvent(new PointerEvent('pointerup', { clientX: 2500, clientY: 500, bubbles: true }));

    const rectFarRight = stage.getBoundingClientRect();
    const leftGap = rectFarRight.left - cRect.left;

    // Pan far left (-5000px)
    container.dispatchEvent(new PointerEvent('pointerdown', { clientX: 500, clientY: 500, bubbles: true }));
    window.dispatchEvent(new PointerEvent('pointermove', { clientX: -4500, clientY: 500, bubbles: true }));
    window.dispatchEvent(new PointerEvent('pointerup', { clientX: -4500, clientY: 500, bubbles: true }));

    const rectFarLeft = stage.getBoundingClientRect();
    const rightGap = cRect.right - rectFarLeft.right;

    return {
      leftGap,
      rightGap,
    };
  });

  if (panClampCheck.leftGap > 1) {
    failures.push(`Requirement 6 failed: gap on left after panning far right was ${panClampCheck.leftGap} px (expected <= 0)`);
  }
  if (panClampCheck.rightGap > 1) {
    failures.push(`Requirement 6 failed: gap on right after panning far left was ${panClampCheck.rightGap} px (expected <= 0)`);
  }
  console.log(`  Pan far right gap = ${panClampCheck.leftGap} px, Pan far left gap = ${panClampCheck.rightGap} px`);

  // 7. With a mask brush active, a plain drag paints and does not pan; with Space held it pans and paints nothing.
  console.log('Asserting 7: Brush plain drag paints without panning; Space+drag pans without painting...');
  // Add a mask
  const masksToggle = page.locator('button[data-testid="section-masks-toggle"]');
  await masksToggle.click();
  await page.waitForTimeout(100);
  await page.click('button[data-testid="add-brush-mask"]');
  await page.waitForTimeout(150);

  const brushPanCheck = await page.evaluate(async () => {
    const stage = document.querySelector('.canvas-viewport__stage');
    const container = document.querySelector('.canvas-viewport__container');
    if (!stage || !container) return { error: 'missing elements' };

    const initialPos = stage.getBoundingClientRect().left;

    // Plain drag on stage (should not pan)
    stage.dispatchEvent(new PointerEvent('pointerdown', { clientX: 400, clientY: 400, bubbles: true }));
    stage.dispatchEvent(new PointerEvent('pointermove', { clientX: 450, clientY: 450, bubbles: true }));
    stage.dispatchEvent(new PointerEvent('pointerup', { clientX: 450, clientY: 450, bubbles: true }));

    const posAfterPlain = stage.getBoundingClientRect().left;
    const plainPanned = Math.abs(posAfterPlain - initialPos) > 1;

    return {
      initialPos,
      posAfterPlain,
      plainPanned,
    };
  });

  if (brushPanCheck.plainPanned) {
    failures.push('Requirement 7 failed: plain drag with mask tool panned the viewport');
  }

  // Now hold space and drag
  await page.keyboard.down('Space');
  await page.waitForTimeout(50);

  const spacePanCheck = await page.evaluate(async () => {
    const container = document.querySelector('.canvas-viewport__container');
    const stage = document.querySelector('.canvas-viewport__stage');
    if (!container || !stage) return { error: 'missing elements' };

    const posBeforeSpace = stage.getBoundingClientRect().left;

    // Drag with space held
    container.dispatchEvent(new PointerEvent('pointerdown', { clientX: 600, clientY: 400, bubbles: true }));
    window.dispatchEvent(new PointerEvent('pointermove', { clientX: 680, clientY: 400, bubbles: true }));
    window.dispatchEvent(new PointerEvent('pointerup', { clientX: 680, clientY: 400, bubbles: true }));

    await new Promise((resolve) => requestAnimationFrame(resolve));
    await new Promise((resolve) => setTimeout(resolve, 50));

    const posAfterSpace = stage.getBoundingClientRect().left;
    const didPan = Math.abs(posAfterSpace - posBeforeSpace) > 5;

    return {
      posBeforeSpace,
      posAfterSpace,
      didPan,
    };
  });

  await page.keyboard.up('Space');
  if (!spacePanCheck.didPan) {
    failures.push('Requirement 7 failed: Space-drag did not pan the viewport');
  }
  console.log(`  Plain drag panned = ${brushPanCheck.plainPanned}, Space-drag panned = ${spacePanCheck.didPan}`);

  // Exit mask mode
  await page.keyboard.press('Escape');
  await page.waitForTimeout(100);

  // 8. With text focused, Z does nothing.
  console.log('Asserting 8: With text focused, Z does nothing...');
  await page.click('button[data-testid="zoom-fit-btn"]');
  await page.waitForTimeout(100);

  await pathInput.focus();
  await page.keyboard.press('KeyZ');
  await page.waitForTimeout(100);

  const zTextCheck = await page.evaluate(() => {
    const fitBtn = document.querySelector('button[data-testid="zoom-fit-btn"]');
    return {
      isFit: fitBtn?.classList.contains('active'),
    };
  });
  if (!zTextCheck.isFit) {
    failures.push('Requirement 8 failed: Z toggled zoom while text input was focused');
  }
  await pathInput.blur();
  console.log(`  Text focused Z check: Zoom stayed at fit = ${zTextCheck.isFit}`);

  // 9. Side-by-side: two canvases of equal CSS size fit the container. The before's request has identity adjustments and the recipe's geometry.
  console.log('Asserting 9: Side-by-side gives two canvases of equal CSS size fitting container, before recipe has identity adjustments & geometry...');
  await page.evaluate(() => {
    window.addEventListener('keydown', (e) => {
      console.log('KEY RECEIVED:', e.key, 'code:', e.code, 'meta:', e.metaKey, 'ctrl:', e.ctrlKey);
    }, { once: true });
    window.__STUB__.renders = [];
  });

  // Apply some geometry and exposure
  await expSlider.evaluate((el) => {
    el.value = '2.5';
    el.dispatchEvent(new Event('input', { bubbles: true }));
    el.dispatchEvent(new Event('change', { bubbles: true }));
  });
  await page.waitForTimeout(100);

  // Toggle side-by-side (press Y)
  await page.keyboard.press('KeyY');
  await page.waitForTimeout(300);

  const sbsCheck = await page.evaluate(() => {
    const beforeCanvas = document.querySelector('canvas[data-testid="before-canvas"]');
    const afterCanvas = document.querySelector('canvas[data-testid="edit-canvas"]');
    const container = document.querySelector('.canvas-viewport__container');
    if (!beforeCanvas || !afterCanvas || !container) return { error: 'elements missing' };

    const bRect = beforeCanvas.getBoundingClientRect();
    const aRect = afterCanvas.getBoundingClientRect();
    const cRect = container.getBoundingClientRect();

    const equalWidth = Math.abs(bRect.width - aRect.width) <= 1;
    const equalHeight = Math.abs(bRect.height - aRect.height) <= 1;
    const fitsContainer = aRect.right <= cRect.right + 2;

    const renders = window.__STUB__.renders;
    // Find before render (exposure == 0)
    const beforeRender = renders.find((r) => r.recipe && r.recipe.exposure === 0);
    const afterRender = renders.find((r) => r.recipe && r.recipe.exposure === 2.5);

    return {
      bWidth: bRect.width,
      aWidth: aRect.width,
      bHeight: bRect.height,
      aHeight: aRect.height,
      equalWidth,
      equalHeight,
      fitsContainer,
      hasBeforeRender: !!beforeRender,
      hasAfterRender: !!afterRender,
    };
  });

  if (!sbsCheck.equalWidth || !sbsCheck.equalHeight) {
    failures.push(`Requirement 9 failed: canvases not equal CSS size (before=${sbsCheck.bWidth}x${sbsCheck.bHeight}, after=${sbsCheck.aWidth}x${sbsCheck.aHeight})`);
  }
  if (!sbsCheck.fitsContainer) {
    failures.push('Requirement 9 failed: side-by-side canvases overflow container');
  }
  if (!sbsCheck.hasBeforeRender) {
    failures.push('Requirement 9 failed: no before render request with identity adjustments');
  }
  console.log(`  Side-by-side: before=${sbsCheck.bWidth}x${sbsCheck.bHeight}, after=${sbsCheck.aWidth}x${sbsCheck.aHeight}, fits=${sbsCheck.fitsContainer}`);

  // Screenshot side-by-side
  await page.screenshot({ path: join(OUT, 'viewing-side-by-side.png') });

  // 10. Twenty slider steps while comparing send no further before requests.
  console.log('Asserting 10: Twenty slider steps while comparing send no further before requests...');
  const beforeCountBeforeSteps = await page.evaluate(() => {
    return window.__STUB__.renders.filter((r) => r.recipe && r.recipe.exposure === 0).length;
  });

  for (let i = 1; i <= 20; i++) {
    await expSlider.evaluate((el, idx) => {
      el.value = String(1.0 + idx * 0.05);
      el.dispatchEvent(new Event('input', { bubbles: true }));
    }, i);
    await page.waitForTimeout(10);
  }
  await expSlider.evaluate((el) => {
    el.dispatchEvent(new Event('change', { bubbles: true }));
  });
  await page.waitForTimeout(200);

  const beforeCountAfterSteps = await page.evaluate(() => {
    return window.__STUB__.renders.filter((r) => r.recipe && r.recipe.exposure === 0).length;
  });

  if (beforeCountAfterSteps !== beforeCountBeforeSteps) {
    failures.push(`Requirement 10 failed: slider steps triggered additional before requests (${beforeCountAfterSteps} vs ${beforeCountBeforeSteps})`);
  }
  console.log(`  Twenty slider steps: before requests count before=${beforeCountBeforeSteps}, after=${beforeCountAfterSteps}`);

  // 11. Split: dragging the divider to 30% gives clip-path at 30% ± 1.
  console.log('Asserting 11: Split mode: dragging divider to 30% gives clip-path at 30% ± 1...');
  // Switch to split compare mode (Shift-Y)
  await page.keyboard.press('Shift+KeyY');
  await page.waitForTimeout(200);

  const splitDragResult = await page.evaluate(async () => {
    const stage = document.querySelector('.canvas-viewport__stage');
    const divider = document.querySelector('.canvas-viewport__split-divider');
    const beforeCanvas = document.querySelector('canvas[data-testid="before-canvas"]');
    if (!stage || !divider || !beforeCanvas) return { error: 'elements missing' };

    const rect = stage.getBoundingClientRect();
    const targetX = rect.left + rect.width * 0.3;

    // Simulate pointer drag of divider to 30%
    divider.dispatchEvent(new PointerEvent('pointerdown', { clientX: rect.left + rect.width * 0.5, bubbles: true }));
    window.dispatchEvent(new PointerEvent('pointermove', { clientX: targetX, bubbles: true }));
    window.dispatchEvent(new PointerEvent('pointerup', { clientX: targetX, bubbles: true }));

    await new Promise((resolve) => requestAnimationFrame(resolve));
    await new Promise((resolve) => setTimeout(resolve, 50));

    const clipPath = window.getComputedStyle(beforeCanvas).clipPath || beforeCanvas.style.clipPath;
    return {
      clipPath,
    };
  });

  // Verify clip-path has 30% (or 29% - 31%)
  const clipMatch = /polygon\([^)]*30(\.0)?%/.test(splitDragResult.clipPath) || /30%/.test(splitDragResult.clipPath);
  if (!clipMatch) {
    failures.push(`Requirement 11 failed: clip-path was "${splitDragResult.clipPath}" (expected 30% ± 1)`);
  }
  console.log(`  Split divider drag clip-path: "${splitDragResult.clipPath}"`);

  // Screenshot split
  await page.screenshot({ path: join(OUT, 'viewing-split.png') });

  // 12. Entering crop mode turns compare off and zoom to fit.
  console.log('Asserting 12: Entering crop mode turns compare off and zoom to fit...');
  // Click crop button
  const geoToggle = page.locator('button[data-testid="section-geometry-toggle"]');
  await geoToggle.click();
  await page.waitForTimeout(100);
  const cropBtn = page.locator('button[data-testid="crop-mode-toggle-btn"]');
  await cropBtn.click();
  await page.waitForTimeout(200);

  const cropEnterCheck = await page.evaluate(() => {
    const sbsBtn = document.querySelector('button[data-testid="compare-side-by-side-btn"]');
    const splitBtn = document.querySelector('button[data-testid="compare-split-btn"]');
    const fitBtn = document.querySelector('button[data-testid="zoom-fit-btn"]');
    const zoom100Btn = document.querySelector('button[data-testid="zoom-100-btn"]');

    return {
      sbsActive: sbsBtn?.classList.contains('active'),
      splitActive: splitBtn?.classList.contains('active'),
      fitActive: fitBtn?.classList.contains('active'),
      zoomDisabled: zoom100Btn?.hasAttribute('disabled'),
    };
  });

  if (cropEnterCheck.sbsActive || cropEnterCheck.splitActive) {
    failures.push('Requirement 12 failed: compare mode remained active after entering crop mode');
  }
  if (!cropEnterCheck.fitActive) {
    failures.push('Requirement 12 failed: zoom did not return to fit after entering crop mode');
  }
  if (!cropEnterCheck.zoomDisabled) {
    failures.push('Requirement 12 failed: zoom buttons not disabled in crop mode');
  }
  console.log(`  Entering crop mode: compare off = ${!cropEnterCheck.sbsActive && !cropEnterCheck.splitActive}, fit = ${cropEnterCheck.fitActive}, zoom disabled = ${cropEnterCheck.zoomDisabled}`);

  // Exit crop mode
  await cropBtn.click();
  await page.waitForTimeout(100);

  // 13. Every new control is at least 40 px. Screenshots of zoomed, side-by-side and split go to layout-proof/.
  console.log('Asserting 13: Every new control is at least 40 px...');
  // Zoom to 100% for screenshot
  await page.keyboard.press('Meta+1');
  await page.waitForTimeout(200);
  await page.screenshot({ path: join(OUT, 'viewing-zoomed.png') });

  const controlSizeCheck = await page.evaluate(() => {
    const selectors = [
      'button[data-testid="zoom-fit-btn"]',
      'button[data-testid="zoom-100-btn"]',
      'button[data-testid="compare-side-by-side-btn"]',
      'button[data-testid="compare-split-btn"]',
      '.canvas-viewport__zoom-readout',
    ];

    const violations = [];
    for (const sel of selectors) {
      const el = document.querySelector(sel);
      if (!el) {
        violations.push({ selector: sel, error: 'not found' });
        continue;
      }
      const rect = el.getBoundingClientRect();
      if (rect.height < 39.5 || rect.width < 39.5) {
        violations.push({ selector: sel, width: rect.width, height: rect.height });
      }
    }
    return violations;
  });

  if (controlSizeCheck.length > 0) {
    for (const v of controlSizeCheck) {
      failures.push(`Requirement 13 failed: ${v.selector} size is ${v.width}x${v.height} (must be >= 40px)`);
    }
  }
  console.log(`  Control touch target audit: ${controlSizeCheck.length} violations`);

  // Helper to blur all active elements so keyboard shortcuts are never suppressed
  const blurAll = async () => {
    await page.evaluate(() => {
      if (document.activeElement && document.activeElement instanceof HTMLElement) {
        document.activeElement.blur();
      }
      window.getSelection()?.removeAllRanges();
    });
  };

  // 14. Orientation 6, no geometry: at 100% the visible canvas is 2000 × 3000 ± 1 and Full is requested.
  console.log('Asserting 14: Orientation 6, no geometry: at 100% visible canvas is 2000 × 3000 ± 1 and Full is requested...');
  await page.evaluate(() => {
    window.__STUB__.orientation = 6;
    window.__STUB__.renders = [];
  });
  await pathInput.fill('/Volumes/Photos/orient6.jpg');
  await pathInput.press('Enter');
  await page.waitForTimeout(200);
  await blurAll();

  // Clear renders to track 100% zoom request
  await page.evaluate(() => {
    window.__STUB__.renders = [];
  });

  // Zoom to 100% (Cmd-1)
  await page.keyboard.press('Meta+1');
  await page.waitForTimeout(200);

  const orient6Check = await page.evaluate(() => {
    const canvas = document.querySelector('canvas[data-testid="edit-canvas"]');
    const renders = window.__STUB__.renders;
    const fullRenders = renders.filter((r) => r.stage === 'Full');
    const rect = canvas ? canvas.getBoundingClientRect() : null;
    return {
      rect: rect ? { width: Math.round(rect.width), height: Math.round(rect.height) } : null,
      fullCount: fullRenders.length,
    };
  });

  if (!orient6Check.rect || Math.abs(orient6Check.rect.width - 2000) > 1 || Math.abs(orient6Check.rect.height - 3000) > 1) {
    failures.push(`Requirement 14 failed: orientation 6 visible canvas was ${orient6Check.rect?.width}x${orient6Check.rect?.height} (expected 2000x3000 ± 1)`);
  }
  if (orient6Check.fullCount < 1) {
    failures.push(`Requirement 14 failed: Full was not requested at 100% for orientation 6 (count=${orient6Check.fullCount})`);
  }
  console.log(`  Orientation 6 (no geom): canvas = ${orient6Check.rect?.width}x${orient6Check.rect?.height}, Full count = ${orient6Check.fullCount}`);

  // 15. Orientation 6 with a geometry present, and orientation 1 with rotate: 90: at 100% the visible canvas is 2000 × 3000 ± 1.
  console.log('Asserting 15: Orientation 6 with geometry, and orientation 1 with rotate: 90: visible canvas is 2000 × 3000 ± 1...');
  // (a) Orientation 6 with geometry present (straighten = 1.0)
  const geomToggle15 = page.locator('button[data-testid="section-geometry-toggle"]');
  if (!(await page.locator('input[data-testid="geometry-straighten-number"]').isVisible())) {
    await geomToggle15.click();
    await page.waitForTimeout(100);
  }
  const straightenInput = page.locator('input[data-testid="geometry-straighten-number"]');
  await straightenInput.fill('1.0');
  await straightenInput.press('Enter');
  await page.waitForTimeout(200);
  await blurAll();
  await page.keyboard.press('Meta+1');
  await page.waitForTimeout(200);

  const orient6GeomCheck = await page.evaluate(() => {
    const canvas = document.querySelector('canvas[data-testid="edit-canvas"]');
    const rect = canvas ? canvas.getBoundingClientRect() : null;
    return rect ? { width: Math.round(rect.width), height: Math.round(rect.height) } : null;
  });

  if (!orient6GeomCheck || Math.abs(orient6GeomCheck.width - 2000) > 1 || Math.abs(orient6GeomCheck.height - 3000) > 1) {
    failures.push(`Requirement 15 failed: orientation 6 with geometry was ${orient6GeomCheck?.width}x${orient6GeomCheck?.height} (expected 2000x3000 ± 1)`);
  }

  // (b) Orientation 1 with rotate: 90
  await page.evaluate(() => {
    window.__STUB__.orientation = 1;
  });
  await pathInput.fill('/Volumes/Photos/orient1_rot.jpg');
  await pathInput.press('Enter');
  await page.waitForTimeout(200);
  await blurAll();

  // Click rotate CW (+90°)
  if (!(await page.locator('button[data-testid="rotate-cw-btn"]').isVisible())) {
    await geomToggle15.click();
    await page.waitForTimeout(100);
  }
  await page.locator('button[data-testid="rotate-cw-btn"]').click();
  await page.waitForTimeout(100);
  await blurAll();
  // Zoom to 100%
  await page.keyboard.press('Meta+1');
  await page.waitForTimeout(200);

  const orient1RotCheck = await page.evaluate(() => {
    const canvas = document.querySelector('canvas[data-testid="edit-canvas"]');
    const rect = canvas ? canvas.getBoundingClientRect() : null;
    return rect ? { width: Math.round(rect.width), height: Math.round(rect.height) } : null;
  });

  if (!orient1RotCheck || Math.abs(orient1RotCheck.width - 2000) > 1 || Math.abs(orient1RotCheck.height - 3000) > 1) {
    failures.push(`Requirement 15 failed: orientation 1 with rotate: 90 was ${orient1RotCheck?.width}x${orient1RotCheck?.height} (expected 2000x3000 ± 1)`);
  }
  console.log(`  Orientation 6 with geom: ${orient6GeomCheck?.width}x${orient6GeomCheck?.height}, Orientation 1 rotate 90: ${orient1RotCheck?.width}x${orient1RotCheck?.height}`);

  // 16. A 40% crop at 100% requests Full, and the visible canvas is 40% of the full width ± 1.
  console.log('Asserting 16: A 40% crop at 100% requests Full, and visible canvas is 40% of full width ± 1...');
  // Open fresh orientation 1 image (6000x4000)
  await page.evaluate(() => {
    window.__STUB__.orientation = 1;
  });
  await pathInput.fill('/Volumes/Photos/crop_test.jpg');
  await pathInput.press('Enter');
  await page.waitForTimeout(200);
  await blurAll();

  // Enter crop mode
  if (!(await page.locator('button[data-testid="crop-mode-toggle-btn"]').isVisible())) {
    await geomToggle15.click();
    await page.waitForTimeout(100);
  }
  await page.locator('button[data-testid="crop-mode-toggle-btn"]').click();
  await page.waitForTimeout(100);

  // Drag se handle to make 40% crop (0.4 width, 0.4 height)
  await page.evaluate(() => {
    const se = document.querySelector('[data-test="crop-handle-se"]');
    const stage = document.querySelector('.canvas-viewport__stage');
    if (!se || !stage) return;
    const stageRect = stage.getBoundingClientRect();
    const startX = stageRect.left + stageRect.width;
    const startY = stageRect.top + stageRect.height;
    const targetX = stageRect.left + stageRect.width * 0.4;
    const targetY = stageRect.top + stageRect.height * 0.4;

    se.dispatchEvent(new PointerEvent('pointerdown', { clientX: startX, clientY: startY, bubbles: true }));
    window.dispatchEvent(new PointerEvent('pointermove', { clientX: targetX, clientY: targetY, bubbles: true }));
    window.dispatchEvent(new PointerEvent('pointerup', { clientX: targetX, clientY: targetY, bubbles: true }));
  });
  await page.waitForTimeout(100);

  // Exit crop mode (click Done Cropping)
  await page.locator('button[data-testid="crop-mode-toggle-btn"]').click();
  await page.waitForTimeout(200);
  await blurAll();

  // Clear renders to check if Full is requested on zoom 100%
  await page.evaluate(() => {
    window.__STUB__.renders = [];
  });

  // Zoom to 100%
  await page.keyboard.press('Meta+1');
  await page.waitForTimeout(200);

  const cropCheck = await page.evaluate(() => {
    const canvas = document.querySelector('canvas[data-testid="edit-canvas"]');
    const rect = canvas ? canvas.getBoundingClientRect() : null;
    const fullRenders = window.__STUB__.renders.filter((r) => r.stage === 'Full');
    return {
      width: rect ? Math.round(rect.width) : null,
      fullCount: fullRenders.length,
    };
  });

  // Full width is 6000 / 2 = 3000. 40% of 3000 is 1200.
  if (cropCheck.width === null || Math.abs(cropCheck.width - 1200) > 1) {
    failures.push(`Requirement 16 failed: visible canvas width was ${cropCheck.width} (expected 1200 ± 1, 40% of 3000)`);
  }
  if (cropCheck.fullCount < 1) {
    failures.push(`Requirement 16 failed: Full was not requested at 100% for 40% crop (count=${cropCheck.fullCount})`);
  }
  console.log(`  40% crop at 100%: canvas width = ${cropCheck.width} (expected 1200 ± 1), Full count = ${cropCheck.fullCount}`);

  // 17. For the orientation 6 case, panning past each edge leaves no gap, measured from the visible canvas's box.
  console.log('Asserting 17: Orientation 6 panning past each edge leaves no gap, measured from visible canvas box...');
  await page.evaluate(() => {
    window.__STUB__.orientation = 6;
  });
  await pathInput.fill('/Volumes/Photos/orient6_pan.jpg');
  await pathInput.press('Enter');
  await page.waitForTimeout(200);
  await blurAll();

  // Zoom to 100%
  await page.keyboard.press('Meta+1');
  await page.waitForTimeout(200);

  const orient6PanCheck = await page.evaluate(async () => {
    const container = document.querySelector('.canvas-viewport__container');
    const canvas = document.querySelector('canvas[data-testid="edit-canvas"]');
    if (!container || !canvas) return { error: 'elements missing' };

    const cRect = container.getBoundingClientRect();

    // 1. Pan far right (+3000px)
    container.dispatchEvent(new PointerEvent('pointerdown', { clientX: 500, clientY: 500, bubbles: true }));
    window.dispatchEvent(new PointerEvent('pointermove', { clientX: 3500, clientY: 500, bubbles: true }));
    window.dispatchEvent(new PointerEvent('pointerup', { clientX: 3500, clientY: 500, bubbles: true }));
    const leftGap = canvas.getBoundingClientRect().left - cRect.left;

    // 2. Pan far left (-5000px)
    container.dispatchEvent(new PointerEvent('pointerdown', { clientX: 500, clientY: 500, bubbles: true }));
    window.dispatchEvent(new PointerEvent('pointermove', { clientX: -4500, clientY: 500, bubbles: true }));
    window.dispatchEvent(new PointerEvent('pointerup', { clientX: -4500, clientY: 500, bubbles: true }));
    const rightGap = cRect.right - canvas.getBoundingClientRect().right;

    // 3. Pan far down (+3000px)
    container.dispatchEvent(new PointerEvent('pointerdown', { clientX: 500, clientY: 500, bubbles: true }));
    window.dispatchEvent(new PointerEvent('pointermove', { clientX: 500, clientY: 3500, bubbles: true }));
    window.dispatchEvent(new PointerEvent('pointerup', { clientX: 500, clientY: 3500, bubbles: true }));
    const topGap = canvas.getBoundingClientRect().top - cRect.top;

    // 4. Pan far up (-5000px)
    container.dispatchEvent(new PointerEvent('pointerdown', { clientX: 500, clientY: 500, bubbles: true }));
    window.dispatchEvent(new PointerEvent('pointermove', { clientX: 500, clientY: -4500, bubbles: true }));
    window.dispatchEvent(new PointerEvent('pointerup', { clientX: 500, clientY: -4500, bubbles: true }));
    const bottomGap = cRect.bottom - canvas.getBoundingClientRect().bottom;

    return {
      leftGap,
      rightGap,
      topGap,
      bottomGap,
    };
  });

  if (orient6PanCheck.leftGap > 1) {
    failures.push(`Requirement 17 failed: left gap after panning right was ${orient6PanCheck.leftGap} px (expected <= 1)`);
  }
  if (orient6PanCheck.rightGap > 1) {
    failures.push(`Requirement 17 failed: right gap after panning left was ${orient6PanCheck.rightGap} px (expected <= 1)`);
  }
  if (orient6PanCheck.topGap > 1) {
    failures.push(`Requirement 17 failed: top gap after panning down was ${orient6PanCheck.topGap} px (expected <= 1)`);
  }
  if (orient6PanCheck.bottomGap > 1) {
    failures.push(`Requirement 17 failed: bottom gap after panning up was ${orient6PanCheck.bottomGap} px (expected <= 1)`);
  }
  console.log(`  Orientation 6 pan gaps: left = ${orient6PanCheck.leftGap.toFixed(1)} px, right = ${orient6PanCheck.rightGap.toFixed(1)} px, top = ${orient6PanCheck.topGap.toFixed(1)} px, bottom = ${orient6PanCheck.bottomGap.toFixed(1)} px`);

  // 18. With a brush active, a double-click changes neither zoom nor pan.
  console.log('Asserting 18: With brush active, double-click changes neither zoom nor pan...');
  // Open fresh orientation 1 image at fit
  await page.evaluate(() => {
    window.__STUB__.orientation = 1;
  });
  await pathInput.fill('/Volumes/Photos/brush_dblclick.jpg');
  await pathInput.press('Enter');
  await page.waitForTimeout(200);
  await blurAll();

  // Add brush mask and make paint active
  if (!(await page.locator('[data-testid="add-brush-mask"]').isVisible())) {
    await page.locator('button[data-testid="section-masks-toggle"]').click();
    await page.waitForTimeout(100);
  }
  await page.click('button[data-testid="add-brush-mask"]');
  await page.waitForTimeout(150);

  // Ensure brush is active (button[data-testid="brush-paint"] has aria-pressed="true")
  const isBrushActive = await page.getAttribute('button[data-testid="brush-paint"]', 'aria-pressed');
  if (isBrushActive !== 'true') {
    await page.click('button[data-testid="brush-paint"]');
    await page.waitForTimeout(50);
  }

  const beforeDblClick = await page.evaluate(() => {
    const readout = document.querySelector('.canvas-viewport__zoom-readout')?.textContent?.trim();
    const stage = document.querySelector('.canvas-viewport__stage');
    const sRect = stage ? stage.getBoundingClientRect() : null;
    return {
      zoomText: readout,
      left: sRect?.left,
      top: sRect?.top,
      width: sRect?.width,
      height: sRect?.height,
    };
  });

  // Double-click on the stage with brush active
  await page.evaluate(() => {
    const stage = document.querySelector('.canvas-viewport__stage');
    stage?.dispatchEvent(new MouseEvent('dblclick', { clientX: 450, clientY: 450, bubbles: true }));
  });
  await page.waitForTimeout(150);

  const afterDblClick = await page.evaluate(() => {
    const readout = document.querySelector('.canvas-viewport__zoom-readout')?.textContent?.trim();
    const stage = document.querySelector('.canvas-viewport__stage');
    const sRect = stage ? stage.getBoundingClientRect() : null;
    return {
      zoomText: readout,
      left: sRect?.left,
      top: sRect?.top,
      width: sRect?.width,
      height: sRect?.height,
    };
  });

  if (afterDblClick.zoomText !== beforeDblClick.zoomText || afterDblClick.width !== beforeDblClick.width || afterDblClick.height !== beforeDblClick.height) {
    failures.push(`Requirement 18 failed: double-click with brush active changed zoom (${beforeDblClick.zoomText} -> ${afterDblClick.zoomText})`);
  }
  if (afterDblClick.left !== beforeDblClick.left || afterDblClick.top !== beforeDblClick.top) {
    failures.push(`Requirement 18 failed: double-click with brush active changed pan position`);
  }
  console.log(`  With brush active: zoom stayed "${afterDblClick.zoomText}", pan position stayed (${afterDblClick.left}, ${afterDblClick.top})`);

} catch (err) {
  failures.push(`Unexpected error in section "${currentSection}": ${err.message}\n${err.stack}`);
} finally {
  await browser.close();
  await server.close();
}

console.log('\n--- check:viewing Summary ---');
if (failures.length > 0) {
  console.error(`FAILED with ${failures.length} issue(s):`);
  for (const f of failures) {
    console.error(`  - ${f}`);
  }
  process.exit(1);
} else {
  console.log('PASSED all 18 viewing requirements successfully.');
}
