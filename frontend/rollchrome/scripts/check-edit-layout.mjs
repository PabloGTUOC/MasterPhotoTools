/**
 * UI-2 acceptance script: verify Edit's layout, resizable adjustments panel,
 * collapsible histogram, popover pickers, toolbar single-row reflow, and spin arrow removal.
 *
 * Checks at 1024×700, 1280×800 and 1920×1080 at panel widths 320 and 560:
 * - (a) .content's scrollHeight <= clientHeight + 1.
 * - (b) No control is clipped in .adjustment-panel.
 * - (c) Export render button and save-status lie inside viewport at scroll top and bottom.
 * - (d) Canvas viewport overlay buttons on a single row (same top ±2px) when canvas >= 560px.
 * - (e) Every input[type="number"] has computed appearance textfield.
 * - (f) Dragging edit-divider clamps to 320 and 560, remembered across reload.
 * - (g) Path field Choose... opens overlay without moving canvas top (±1), Escape closes it.
 * - (h) Histogram toggle hides plot, state survives reload.
 *
 * Screenshots written to layout-proof/edit-layout-<width>-<min|max>.png.
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
const harness = fileURLToPath(new URL('./app-harness', import.meta.url));
const stubApi = fileURLToPath(new URL('./app-harness/stub-api.ts', import.meta.url));
const hostSrc = fileURLToPath(new URL('../src', import.meta.url));

console.log('Starting Vite server for application shell harness (check:edit-layout)...');
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
  viewport: { width: 1024, height: 700 },
  deviceScaleFactor: 2,
});
page.setDefaultTimeout(15000);

const failures = [];
let passedAssertions = 0;

function assert(condition, message) {
  if (condition) {
    passedAssertions++;
  } else {
    failures.push(message);
    console.error(`  FAIL: ${message}`);
  }
}

const SIZES = [
  { width: 1024, height: 700 },
  { width: 1280, height: 800 },
  { width: 1920, height: 1080 },
];

const watchdog = setTimeout(() => {
  console.error('\ncheck:edit-layout FAILED: watchdog timeout after 8 minutes');
  process.exit(1);
}, 8 * 60 * 1000);
watchdog.unref();

async function setPanelWidthViaDrag(targetWidth) {
  const divider = page.locator('[data-testid="edit-divider"]');
  const box = await divider.boundingBox();
  if (!box) throw new Error('Divider bounding box not found');
  const currentWidth = await page.evaluate(() => {
    return document.querySelector('.adjustment-panel')?.getBoundingClientRect().width ?? 380;
  });
  // Panel is on right. Moving divider left expands panel, moving right narrows panel.
  // delta = currentWidth - targetWidth (if delta > 0, panel must shrink -> move divider right by delta; if delta < 0, panel must grow -> move divider left)
  const moveX = (currentWidth - targetWidth);
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width / 2 + moveX, box.y + box.height / 2);
  await page.mouse.up();
  await page.waitForTimeout(100);
}

try {
  // Clear any existing localStorage keys first
  await page.goto(`${base}/#/edit`, { waitUntil: 'networkidle' });
  await page.evaluate(() => {
    localStorage.removeItem('rollchrome.edit.panelWidth');
    localStorage.removeItem('rollchrome.edit.histogramOpen');
  });

  for (const size of SIZES) {
    console.log(`\nTesting window size ${size.width}×${size.height}...`);
    await page.setViewportSize({ width: size.width, height: size.height });

    // 1. Open #/edit and load /Volumes/Photos/sample.jpg as check-shell.mjs does
    await page.goto(`${base}/#/edit`, { waitUntil: 'networkidle' });
    const editPathInput = page.locator('input[placeholder="/path/to/image.jpg"]');
    await editPathInput.fill('/Volumes/Photos/sample.jpg');
    await editPathInput.press('Enter');

    await page.waitForSelector('canvas[data-testid="edit-canvas"]', { timeout: 10000 });
    await page.waitForTimeout(200);

    // 2. Expand every section: click .adjustment-panel .collapsible-section__toggle[aria-expanded="false"] until none left
    while (true) {
      const collapsibles = page.locator('.adjustment-panel .collapsible-section__toggle[aria-expanded="false"]');
      const count = await collapsibles.count();
      if (count === 0) break;
      await collapsibles.first().click();
      await page.waitForTimeout(50);
    }

    // 3. Do the checks with the panel at its minimum (320), then at its maximum (560)
    for (const targetWidth of [320, 560]) {
      const mode = targetWidth === 320 ? 'min' : 'max';
      console.log(`  Checking panel at ${mode} (${targetWidth}px)...`);

      // Set width by dragging edit-divider
      await setPanelWidthViaDrag(targetWidth);

      const panelBox = await page.locator('.adjustment-panel').boundingBox();
      assert(
        panelBox && Math.abs(panelBox.width - targetWidth) <= 1.5,
        `[${size.width}×${size.height} ${mode}] Panel width is ${panelBox?.width}px, expected ${targetWidth} (±1.5)`
      );

      // (a) .content's scrollHeight <= clientHeight + 1
      const contentMetrics = await page.evaluate(() => {
        const content = document.querySelector('.content');
        if (!content) return null;
        return {
          scrollHeight: content.scrollHeight,
          clientHeight: content.clientHeight,
        };
      });
      assert(
        contentMetrics && contentMetrics.scrollHeight <= contentMetrics.clientHeight + 1,
        `[${size.width}×${size.height} ${mode}] .content scrollHeight (${contentMetrics?.scrollHeight}) exceeds clientHeight + 1 (${(contentMetrics?.clientHeight ?? 0) + 1})`
      );

      // (b) No control is clipped in .adjustment-panel
      const clippedControls = await page.evaluate(() => {
        const offenders = [];
        const panel = document.querySelector('.adjustment-panel');
        if (!panel) return [{ tag: 'none', className: 'none', text: 'no .adjustment-panel' }];

        const elements = panel.querySelectorAll('button, input, select, textarea, label, canvas, svg');
        for (const el of elements) {
          const rect = el.getBoundingClientRect();
          if (rect.width <= 0 || rect.height <= 0) continue;

          // Find nearest ancestor whose overflow or overflowX is auto, scroll, hidden, clip
          let ancestor = el.parentElement;
          let clipAncestor = null;
          while (ancestor) {
            const style = window.getComputedStyle(ancestor);
            const ov = style.overflow;
            const ovX = style.overflowX;
            if (['auto', 'scroll', 'hidden', 'clip'].includes(ov) || ['auto', 'scroll', 'hidden', 'clip'].includes(ovX)) {
              clipAncestor = ancestor;
              break;
            }
            ancestor = ancestor.parentElement;
          }

          if (clipAncestor) {
            const aRect = clipAncestor.getBoundingClientRect();
            const leftOver = rect.left < aRect.left - 1.5;
            const rightOver = rect.right > aRect.right + 1.5;
            if (leftOver || rightOver) {
              offenders.push({
                tag: el.tagName.toLowerCase(),
                className: el.className,
                text: (el.textContent || '').trim().slice(0, 30),
                rect: { left: Math.round(rect.left), right: Math.round(rect.right), width: Math.round(rect.width) },
                ancestorRect: { left: Math.round(aRect.left), right: Math.round(aRect.right), width: Math.round(aRect.width) },
                offense: leftOver ? 'left-clipped' : 'right-clipped',
              });
            }
          }
        }
        return offenders;
      });

      if (clippedControls.length > 0) {
        for (const off of clippedControls) {
          console.error(`  Offender: <${off.tag} class="${off.className}"> "${off.text}" - ${off.offense} (rect.right=${off.rect.right}, ancestor.right=${off.ancestorRect.right})`);
        }
      }
      assert(
        clippedControls.length === 0,
        `[${size.width}×${size.height} ${mode}] Found ${clippedControls.length} clipped controls in .adjustment-panel`
      );

      // (c) With panel body scrolled to top and then bottom, Export render button and save-status lie inside viewport
      for (const scrollPos of ['top', 'bottom']) {
        await page.evaluate((pos) => {
          const panel = document.querySelector('.adjustment-panel');
          if (panel) {
            panel.scrollTop = pos === 'top' ? 0 : panel.scrollHeight;
          }
        }, scrollPos);
        await page.waitForTimeout(50);

        const footerMetrics = await page.evaluate(() => {
          const exportBtn = document.querySelector('[data-testid="export-btn"]');
          const saveStatus = document.querySelector('[data-testid="save-status"]');
          if (!exportBtn || !saveStatus) return { found: false };

          const eRect = exportBtn.getBoundingClientRect();
          const sRect = saveStatus.getBoundingClientRect();
          const vHeight = window.innerHeight;
          const vWidth = window.innerWidth;

          const eInside = eRect.top >= 0 && eRect.bottom <= vHeight && eRect.left >= 0 && eRect.right <= vWidth;
          const sInside = sRect.top >= 0 && sRect.bottom <= vHeight && sRect.left >= 0 && sRect.right <= vWidth;

          return {
            found: true,
            exportInside: eInside,
            saveStatusInside: sInside,
            eRect: { top: Math.round(eRect.top), bottom: Math.round(eRect.bottom) },
            sRect: { top: Math.round(sRect.top), bottom: Math.round(sRect.bottom) },
            vHeight,
          };
        });

        assert(
          footerMetrics.found && footerMetrics.exportInside && footerMetrics.saveStatusInside,
          `[${size.width}×${size.height} ${mode} scroll=${scrollPos}] Export render button or save-status outside viewport: ${JSON.stringify(footerMetrics)}`
        );
      }

      // (d) At canvas widths >= 560 px, all of .canvas-viewport__overlay's buttons have the same top, ±2 px
      const overlayMetrics = await page.evaluate(() => {
        const canvasEl = document.querySelector('.canvas-viewport');
        if (!canvasEl) return null;
        const canvasWidth = canvasEl.getBoundingClientRect().width;
        const buttons = [...document.querySelectorAll('.canvas-viewport__overlay button')];
        const tops = buttons.map((b) => b.getBoundingClientRect().top);
        return {
          canvasWidth,
          buttonCount: buttons.length,
          tops,
          diff: tops.length > 0 ? Math.max(...tops) - Math.min(...tops) : 0,
        };
      });

      if (overlayMetrics && overlayMetrics.canvasWidth >= 560) {
        assert(
          overlayMetrics.diff <= 2,
          `[${size.width}×${size.height} ${mode}] Canvas width ${overlayMetrics.canvasWidth}px >= 560px, but overlay buttons tops diff is ${overlayMetrics.diff}px (expected <= 2px)`
        );
      }

      // (e) Every input[type="number"] has computed appearance equal to textfield
      const numberInputsAppearance = await page.evaluate(() => {
        const inputs = [...document.querySelectorAll('input[type="number"]')];
        return inputs.map((inp) => ({
          testId: inp.getAttribute('data-testid') || '',
          appearance: window.getComputedStyle(inp).appearance,
        }));
      });
      const nonTextfield = numberInputsAppearance.filter((i) => i.appearance !== 'textfield');
      assert(
        nonTextfield.length === 0,
        `[${size.width}×${size.height} ${mode}] Number inputs with appearance != textfield: ${JSON.stringify(nonTextfield)}`
      );

      // Screenshot to layout-proof/edit-layout-<width>-<min|max>.png
      const screenshotPath = join(OUT, `edit-layout-${size.width}-${mode}.png`);
      await page.screenshot({ path: screenshotPath });
    }
  }

  // =========================================================================
  // Detailed Functional Assertions: (f), (g), (h)
  // =========================================================================
  console.log('\nAsserting divider clamping and memory (f)...');
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto(`${base}/#/edit`, { waitUntil: 'networkidle' });
  const pathInpF = page.locator('input[placeholder="/path/to/image.jpg"]');
  await pathInpF.fill('/Volumes/Photos/sample.jpg');
  await pathInpF.press('Enter');
  await page.waitForSelector('canvas[data-testid="edit-canvas"]', { timeout: 10000 });
  await page.waitForTimeout(200);

  // Drag divider by +2000 px (mouse moves right -> panel clamps to 320)
  const divEl = page.locator('[data-testid="edit-divider"]');
  let divBox = await divEl.boundingBox();
  assert(!!divBox, 'Divider bounding box found');
  if (divBox) {
    await page.mouse.move(divBox.x + divBox.width / 2, divBox.y + divBox.height / 2);
    await page.mouse.down();
    await page.mouse.move(divBox.x + divBox.width / 2 + 2000, divBox.y + divBox.height / 2);
    await page.mouse.up();
    await page.waitForTimeout(100);

    const w320 = await page.evaluate(() => document.querySelector('.adjustment-panel')?.getBoundingClientRect().width);
    assert(Math.abs((w320 || 0) - 320) <= 1, `Dragging edit-divider by +2000 clamps panel to 320 (±1), got ${w320}`);

    // Drag divider by -2000 px (mouse moves left -> panel clamps to 560)
    divBox = await divEl.boundingBox();
    if (divBox) {
      await page.mouse.move(divBox.x + divBox.width / 2, divBox.y + divBox.height / 2);
      await page.mouse.down();
      await page.mouse.move(divBox.x + divBox.width / 2 - 2000, divBox.y + divBox.height / 2);
      await page.mouse.up();
      await page.waitForTimeout(100);

      const w560 = await page.evaluate(() => document.querySelector('.adjustment-panel')?.getBoundingClientRect().width);
      assert(Math.abs((w560 || 0) - 560) <= 1, `Dragging edit-divider by -2000 clamps panel to 560 (±1), got ${w560}`);

      // Reload and reopen photograph, verify last width is kept
      await page.reload({ waitUntil: 'networkidle' });
      const reloadInp = page.locator('input[placeholder="/path/to/image.jpg"]');
      await reloadInp.fill('/Volumes/Photos/sample.jpg');
      await reloadInp.press('Enter');
      await page.waitForSelector('canvas[data-testid="edit-canvas"]', { timeout: 10000 });
      await page.waitForTimeout(100);

      const wReload = await page.evaluate(() => document.querySelector('.adjustment-panel')?.getBoundingClientRect().width);
      assert(Math.abs((wReload || 0) - 560) <= 1, `After reload and reopening photo, panel width 560 is kept, got ${wReload}`);

      // Clear key at the end
      await page.evaluate(() => localStorage.removeItem('rollchrome.edit.panelWidth'));
    }
  }

  // (g) Path field Choose... overlay assertion
  console.log('Asserting PathField overlay popover (g)...');
  const canvasTopBefore = await page.evaluate(() => {
    return document.querySelector('canvas[data-testid="edit-canvas"]')?.getBoundingClientRect().top ?? 0;
  });

  const chooseBtn = page.locator('[data-testid="path-browse"]').first();
  await chooseBtn.click();
  await page.waitForTimeout(100);

  const canvasTopAfter = await page.evaluate(() => {
    return document.querySelector('canvas[data-testid="edit-canvas"]')?.getBoundingClientRect().top ?? 0;
  });

  assert(
    Math.abs(canvasTopAfter - canvasTopBefore) <= 1,
    `Clicking Choose... moved canvas top: before=${canvasTopBefore}, after=${canvasTopAfter}`
  );

  const pickerEl = page.locator('.picker');
  assert(await pickerEl.isVisible(), 'FolderPicker should be visible after clicking Choose...');

  await page.keyboard.press('Escape');
  await page.waitForTimeout(100);
  assert(!(await pickerEl.isVisible()), 'FolderPicker should be closed after pressing Escape');

  // (h) Histogram toggle hides plot and survives reload
  console.log('Asserting Histogram toggle and memory (h)...');
  const histToggle = page.locator('[data-testid="histogram-toggle"]');
  const histPlot = page.locator('.adjustment-panel__histogram [data-testid="histogram-plot"]');

  assert(await histPlot.isVisible(), 'Histogram plot is visible initially');

  await histToggle.click();
  await page.waitForTimeout(100);
  assert(!(await histPlot.isVisible()), 'Histogram plot is hidden after clicking toggle');

  // Reload and reopen photograph
  await page.reload({ waitUntil: 'networkidle' });
  const histInp = page.locator('input[placeholder="/path/to/image.jpg"]');
  await histInp.fill('/Volumes/Photos/sample.jpg');
  await histInp.press('Enter');
  await page.waitForSelector('canvas[data-testid="edit-canvas"]', { timeout: 10000 });
  await page.waitForTimeout(100);

  const histPlotReload = page.locator('.adjustment-panel__histogram [data-testid="histogram-plot"]');
  assert(!(await histPlotReload.isVisible()), 'Histogram plot remains hidden after reload');

  // Clear key at end
  await page.evaluate(() => localStorage.removeItem('rollchrome.edit.histogramOpen'));

} catch (err) {
  failures.push(`Execution error: ${err.message}\n${err.stack}`);
  console.error(err);
} finally {
  clearTimeout(watchdog);
  await browser.close();
  await server.close();
}

console.log(`\n${passedAssertions} assertions passed.`);
if (failures.length > 0) {
  console.error(`\nFAILED with ${failures.length} issues:`);
  for (const f of failures) console.error(`  - ${f}`);
  process.exit(1);
}
