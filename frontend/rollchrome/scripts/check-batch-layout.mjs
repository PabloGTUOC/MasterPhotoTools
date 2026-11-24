/**
 * UI-5 acceptance script: verify Batch's two-column layout, fixed action footer,
 * resizable settings panel, status strip drawer inset, and absence of page/content scroll.
 *
 * Runs on the whole-application harness at 1024×700 and 1920×1080.
 *
 * Checks at 1024×700 and 1920×1080:
 * - (a) .content's scrollHeight <= clientHeight + 1.
 * - (b) dry-run-button, the run button and gate-explanation lie inside the viewport.
 * - (c) with every source chosen (LUT and Preset), as check-bulk-lut.mjs fills them,
 *       no control inside the screen is clipped by its nearest clipping ancestor
 *       (the test check-edit-layout.mjs uses, reused) at widths 320 and 560.
 * - (d) the divider clamps to 320 and 560 and is remembered across reload.
 * - (e) after a dry run, the drawer's right edge is at or left of the right column's left edge (±1).
 *
 * Writes screenshots to layout-proof/batch-layout-<width>.png.
 * Prints "N assertions passed".
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

console.log('Starting Vite harness server for application shell (check:batch-layout)...');
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
  { width: 1920, height: 1080 },
];

const watchdog = setTimeout(() => {
  console.error('\ncheck:batch-layout FAILED: watchdog timeout after 8 minutes');
  process.exit(1);
}, 8 * 60 * 1000);
watchdog.unref();

function isInsideViewport(box, size) {
  if (!box) return false;
  return (
    box.x >= 0 &&
    box.y >= 0 &&
    box.x + box.width <= size.width + 1.5 &&
    box.y + box.height <= size.height + 1.5
  );
}

async function assertContentNoScroll(context) {
  const metrics = await page.evaluate(() => {
    const content = document.querySelector('.content');
    if (!content) return null;
    return {
      scrollHeight: content.scrollHeight,
      clientHeight: content.clientHeight,
    };
  });
  assert(
    metrics && metrics.scrollHeight <= metrics.clientHeight + 1,
    `[${context}] .content scrollHeight (${metrics?.scrollHeight}) exceeds clientHeight + 1 (${(metrics?.clientHeight ?? 0) + 1})`
  );
}

async function setSettingsWidthViaDrag(targetWidth) {
  const divider = page.locator('[data-testid="batch-settings-divider"]');
  const box = await divider.boundingBox();
  if (!box) throw new Error('Divider bounding box not found');
  const currentWidth = await page.evaluate(() => {
    return document.querySelector('.batch-settings-column')?.getBoundingClientRect().width ?? 360;
  });
  // Right column: moving divider left (negative moveX) widens it, moving right narrows it
  const moveX = currentWidth - targetWidth;
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width / 2 + moveX, box.y + box.height / 2);
  await page.mouse.up();
  await page.waitForTimeout(100);
}

async function checkNoClippedControls(context) {
  const clippedControls = await page.evaluate(() => {
    const offenders = [];
    const elements = document.querySelectorAll(
      '.bulk-lut-page button, .bulk-lut-page input, .bulk-lut-page select, .bulk-lut-page textarea, .bulk-lut-page label, .bulk-lut-page canvas, .bulk-lut-page svg'
    );
    for (const el of elements) {
      const rect = el.getBoundingClientRect();
      if (rect.width <= 0 || rect.height <= 0) continue;

      let ancestor = el.parentElement;
      let clipAncestor = null;
      while (ancestor) {
        const style = window.getComputedStyle(ancestor);
        const ov = style.overflow;
        const ovX = style.overflowX;
        if (
          ['auto', 'scroll', 'hidden', 'clip'].includes(ov) ||
          ['auto', 'scroll', 'hidden', 'clip'].includes(ovX)
        ) {
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
            rect: {
              left: Math.round(rect.left),
              right: Math.round(rect.right),
              width: Math.round(rect.width),
            },
            ancestorRect: {
              left: Math.round(aRect.left),
              right: Math.round(aRect.right),
              width: Math.round(aRect.width),
            },
            offense: leftOver ? 'left-clipped' : 'right-clipped',
          });
        }
      }
    }
    return offenders;
  });

  if (clippedControls.length > 0) {
    for (const off of clippedControls) {
      console.error(
        `  Offender: <${off.tag} class="${off.className}"> "${off.text}" - ${off.offense} (rect.right=${off.rect.right}, ancestor.right=${off.ancestorRect.right})`
      );
    }
  }
  assert(
    clippedControls.length === 0,
    `[${context}] Found ${clippedControls.length} clipped controls`
  );
}

try {
  for (const size of SIZES) {
    console.log(`\nTesting window size ${size.width}×${size.height}...`);
    await page.setViewportSize({ width: size.width, height: size.height });

    // Navigate to Batch view and clear any remembered width
    await page.goto(`${base}/#/batch`, { waitUntil: 'networkidle' });
    await page.evaluate(() => localStorage.removeItem('rollchrome.batch.settingsWidth'));
    await page.reload({ waitUntil: 'networkidle' });
    await page.waitForSelector('.bulk-lut-page', { timeout: 10000 });
    await page.waitForTimeout(200);

    // 1. Initial state checks before settings are filled
    console.log(`  [${size.width}×${size.height}] Checking initial layout and controls in viewport...`);
    await assertContentNoScroll(`Initial ${size.width}×${size.height}`);

    const dryRunBtn = page.locator('[data-testid="dry-run-button"]');
    const runBtn = page.locator('[data-testid="run-button"]');
    const gateExpl = page.locator('[data-testid="gate-explanation"]');

    const dryRunBox = await dryRunBtn.boundingBox();
    const runBox = await runBtn.boundingBox();
    const gateBox = await gateExpl.boundingBox();

    assert(
      isInsideViewport(dryRunBox, size),
      `[Initial ${size.width}×${size.height}] dry-run-button must lie inside viewport`
    );
    assert(
      isInsideViewport(runBox, size),
      `[Initial ${size.width}×${size.height}] run-button must lie inside viewport`
    );
    assert(
      isInsideViewport(gateBox, size),
      `[Initial ${size.width}×${size.height}] gate-explanation must lie inside viewport`
    );

    // 2. Divider clamping and persistence
    console.log(`  [${size.width}×${size.height}] Testing divider clamping and persistence...`);
    const divEl = page.locator('[data-testid="batch-settings-divider"]');
    let divBox = await divEl.boundingBox();
    assert(Boolean(divBox), `[${size.width}×${size.height}] batch-settings-divider exists`);

    if (divBox) {
      // Drag divider far right (+2000px): clamps to 320
      await page.mouse.move(divBox.x + divBox.width / 2, divBox.y + divBox.height / 2);
      await page.mouse.down();
      await page.mouse.move(divBox.x + divBox.width / 2 + 2000, divBox.y + divBox.height / 2);
      await page.mouse.up();
      await page.waitForTimeout(100);

      const w320 = await page.evaluate(
        () => document.querySelector('.batch-settings-column')?.getBoundingClientRect().width,
      );
      assert(
        Math.abs((w320 || 0) - 320) <= 1.5,
        `[${size.width}×${size.height}] Dragging divider far right clamps to 320 (±1.5), got ${w320}`,
      );

      // Drag divider far left (-2000px): clamps to 560
      divBox = await divEl.boundingBox();
      if (divBox) {
        await page.mouse.move(divBox.x + divBox.width / 2, divBox.y + divBox.height / 2);
        await page.mouse.down();
        await page.mouse.move(divBox.x + divBox.width / 2 - 2000, divBox.y + divBox.height / 2);
        await page.mouse.up();
        await page.waitForTimeout(100);

        const w560 = await page.evaluate(
          () => document.querySelector('.batch-settings-column')?.getBoundingClientRect().width,
        );
        assert(
          Math.abs((w560 || 0) - 560) <= 1.5,
          `[${size.width}×${size.height}] Dragging divider far left clamps to 560 (±1.5), got ${w560}`,
        );

        // Set to custom width 420 and verify persistence
        await setSettingsWidthViaDrag(420);
        const w420 = await page.evaluate(
          () => document.querySelector('.batch-settings-column')?.getBoundingClientRect().width,
        );
        assert(
          Math.abs((w420 || 0) - 420) <= 1.5,
          `[${size.width}×${size.height}] Panel width set to 420 (±1.5), got ${w420}`,
        );

        await page.reload({ waitUntil: 'networkidle' });
        await page.waitForTimeout(100);

        const wReload = await page.evaluate(
          () => document.querySelector('.batch-settings-column')?.getBoundingClientRect().width,
        );
        assert(
          Math.abs((wReload || 0) - 420) <= 1.5,
          `[${size.width}×${size.height}] After reload, panel width 420 is remembered (±1.5), got ${wReload}`,
        );
      }
    }

    // 3. Source: 3D LUT (as check-bulk-lut.mjs fills them)
    console.log(`  [${size.width}×${size.height}] Testing 3D LUT source...`);
    const inputsArea = page.locator('textarea');
    await inputsArea.fill('/Volumes/Photos\n/Volumes/Photos/single.jpg');

    const lutSelect = page.locator('select').first();
    await lutSelect.selectOption({ index: 1 });

    const destInput = page.locator('input[placeholder="/path/to/destination"]');
    await destInput.fill('/Volumes/Photos/graded_output');
    await page.waitForTimeout(100);

    for (const targetWidth of [320, 560]) {
      console.log(`    Testing 3D LUT source at width ${targetWidth}...`);
      await setSettingsWidthViaDrag(targetWidth);
      await assertContentNoScroll(`3D LUT ${size.width}×${size.height} w=${targetWidth}`);
      await checkNoClippedControls(`3D LUT ${size.width}×${size.height} w=${targetWidth}`);

      const bRunBox = await runBtn.boundingBox();
      const bDryBox = await dryRunBtn.boundingBox();
      assert(
        isInsideViewport(bRunBox, size),
        `[3D LUT ${size.width}×${size.height} w=${targetWidth}] run-button must lie inside viewport`,
      );
      assert(
        isInsideViewport(bDryBox, size),
        `[3D LUT ${size.width}×${size.height} w=${targetWidth}] dry-run-button must lie inside viewport`,
      );
    }

    // Run dry run with 3D LUT
    console.log(`    Running dry run with 3D LUT...`);
    await dryRunBtn.click();

    const drawer = page.locator('#status-strip-details');
    const reviewSection = page.locator('[data-testid="review-section"]');
    await reviewSection.waitFor({ state: 'visible', timeout: 5000 });
    await page.waitForFunction(
      () => !document.querySelector('[data-testid="dry-run-button"]')?.textContent?.includes('Planning'),
      { timeout: 5000 },
    );
    await page.waitForTimeout(100);

    // Assert details drawer opens beside right column, never under its footer
    const drawerBox = await drawer.boundingBox();
    const rightColBox = await page.locator('.batch-settings-column').boundingBox();
    assert(
      Boolean(drawerBox && rightColBox),
      `[3D LUT dry run ${size.width}×${size.height}] Drawer and right column bounding boxes exist`,
    );
    if (drawerBox && rightColBox) {
      const drawerRight = drawerBox.x + drawerBox.width;
      const rightColLeft = rightColBox.x;
      assert(
        drawerRight <= rightColLeft + 1.5,
        `[3D LUT dry run ${size.width}×${size.height}] Drawer right edge (${drawerRight.toFixed(1)}) at or left of right column left edge (${rightColLeft.toFixed(1)} ±1)`,
      );
    }

    await assertContentNoScroll(`3D LUT after dry run ${size.width}×${size.height}`);
    const postDryRunBox = await runBtn.boundingBox();
    assert(
      isInsideViewport(postDryRunBox, size),
      `[3D LUT after dry run ${size.width}×${size.height}] run-button must lie inside viewport`,
    );

    // 4. Source: Preset (as check-bulk-lut.mjs fills them)
    console.log(`  [${size.width}×${size.height}] Testing Preset source...`);
    const presetBtn = page.locator('[data-testid="source-preset"]');
    await presetBtn.click();
    await page.waitForTimeout(100);

    const presetSelect = page.locator('[data-testid="batch-preset-select"]');
    await presetSelect.selectOption('Warm Film');
    await page.waitForTimeout(100);

    for (const targetWidth of [320, 560]) {
      console.log(`    Testing Preset source at width ${targetWidth}...`);
      await setSettingsWidthViaDrag(targetWidth);
      await assertContentNoScroll(`Preset ${size.width}×${size.height} w=${targetWidth}`);
      await checkNoClippedControls(`Preset ${size.width}×${size.height} w=${targetWidth}`);

      const pRunBox = await runBtn.boundingBox();
      const pDryBox = await dryRunBtn.boundingBox();
      assert(
        isInsideViewport(pRunBox, size),
        `[Preset ${size.width}×${size.height} w=${targetWidth}] run-button must lie inside viewport`,
      );
      assert(
        isInsideViewport(pDryBox, size),
        `[Preset ${size.width}×${size.height} w=${targetWidth}] dry-run-button must lie inside viewport`,
      );
    }

    // Run dry run with Preset
    console.log(`    Running dry run with Preset...`);
    await dryRunBtn.click();
    await reviewSection.waitFor({ state: 'visible', timeout: 5000 });
    await page.waitForFunction(
      () => !document.querySelector('[data-testid="dry-run-button"]')?.textContent?.includes('Planning'),
      { timeout: 5000 },
    );
    await page.waitForTimeout(100);

    const pDrawerBox = await drawer.boundingBox();
    const pRightColBox = await page.locator('.batch-settings-column').boundingBox();
    assert(
      Boolean(pDrawerBox && pRightColBox),
      `[Preset dry run ${size.width}×${size.height}] Drawer and right column bounding boxes exist`,
    );
    if (pDrawerBox && pRightColBox) {
      const drawerRight = pDrawerBox.x + pDrawerBox.width;
      const rightColLeft = pRightColBox.x;
      assert(
        drawerRight <= rightColLeft + 1.5,
        `[Preset dry run ${size.width}×${size.height}] Drawer right edge (${drawerRight.toFixed(1)}) at or left of right column left edge (${rightColLeft.toFixed(1)} ±1)`,
      );
    }

    await assertContentNoScroll(`Preset after dry run ${size.width}×${size.height}`);

    // Screenshot
    const shotPath = join(OUT, `batch-layout-${size.width}.png`);
    await page.screenshot({ path: shotPath });
    console.log(`  Wrote screenshot: ${shotPath}`);

    // Clear key at the end of each size
    await page.evaluate(() => localStorage.removeItem('rollchrome.batch.settingsWidth'));
  }
} finally {
  await browser.close();
  await server.close();
}

console.log(`\n${passedAssertions} assertions passed`);

if (failures.length > 0) {
  console.error(`\ncheck:batch-layout FAILED with ${failures.length} failure(s):`);
  for (const f of failures) {
    console.error(`  - ${f}`);
  }
  process.exit(1);
} else {
  console.log('check:batch-layout PASSED cleanly.');
}
