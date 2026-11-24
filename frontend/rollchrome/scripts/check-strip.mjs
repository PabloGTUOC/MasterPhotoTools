/**
 * UI-6 acceptance check: verify status strip, drawer, compact progress, and fixed action buttons.
 *
 * Runs on the whole-application harness at 1024×700 and 1920×1080.
 *
 * Tests:
 * 1. Develop:
 *    - Open folder, select photo, click dry-run-button.
 *    - status-line reads "Dry run · … to develop", and lies inside viewport.
 *    - The drawer is open and review-section is inside it, inside viewport.
 *    - run-button is inside viewport and enabled.
 *    - .content scrollTop is 0.
 *    - Click run-button: compact progress with Cancel button inside viewport.
 * 2. Escape closes the drawer. The toggle reopens it.
 * 3. Switching to Edit and back clears the line and closes the drawer.
 * 4. Batch:
 *    - Fill inputs and click dry-run-button.
 *    - status-line reads "Dry run · … to grade", inside viewport.
 *    - drawer is open, review-section inside drawer and inside viewport.
 *    - run-button inside viewport and enabled.
 * 5. No page or content scroll needed at any point.
 *
 * Writes screenshots to layout-proof/strip-<screen>-<width>.png.
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

function isInsideViewport(box, size) {
  if (!box) return false;
  return (
    box.x >= -1 &&
    box.y >= -1 &&
    box.x + box.width <= size.width + 1 &&
    box.y + box.height <= size.height + 1
  );
}

async function assertNoScroll(contextMsg) {
  const scrollMetrics = await page.evaluate(() => {
    const doc = document.scrollingElement || document.documentElement;
    const docScroll = doc ? doc.scrollTop : 0;
    const content = document.querySelector('.content');
    const contentScroll = content ? content.scrollTop : 0;
    return { docScroll, contentScroll };
  });
  assert(
    scrollMetrics.docScroll === 0,
    `[${contextMsg}] document.scrollingElement.scrollTop is ${scrollMetrics.docScroll}, expected 0`
  );
  assert(
    scrollMetrics.contentScroll === 0,
    `[${contextMsg}] .content scrollTop is ${scrollMetrics.contentScroll}, expected 0`
  );
}

const SIZES = [
  { width: 1024, height: 700 },
  { width: 1920, height: 1080 },
];

const watchdog = setTimeout(() => {
  console.error('\ncheck:strip FAILED: watchdog timeout after 5 minutes');
  process.exit(1);
}, 5 * 60 * 1000);
watchdog.unref();

try {
  for (const size of SIZES) {
    console.log(`\nTesting status strip at ${size.width}×${size.height}...`);
    await page.setViewportSize({ width: size.width, height: size.height });

    // =========================================================================
    // 1. Develop
    // =========================================================================
    console.log(`  1. Develop at ${size.width}×${size.height}...`);
    await page.goto(`${base}/#/`, { waitUntil: 'networkidle' });
    const devFolderInput = page.locator('input[placeholder="/path/to/raw/photos"]');
    await devFolderInput.fill('/Volumes/Photos/RAW');
    await devFolderInput.press('Enter');

    await page.waitForSelector('.thumbnail-card', { timeout: 10000 });
    await page.locator('.thumbnail-card').first().click();
    await page.waitForTimeout(200);

    await assertNoScroll(`Develop loaded ${size.width}×${size.height}`);

    // Click dry-run-button
    const dryRunBtn = page.locator('[data-testid="dry-run-button"]');
    await dryRunBtn.click();

    // Wait for status-line and review-section
    const statusLine = page.locator('[data-testid="status-line"]');
    await statusLine.waitFor({ state: 'visible', timeout: 5000 });
    const statusText = (await statusLine.textContent()) ?? '';
    assert(
      statusText.includes('Dry run · ') && statusText.includes('to develop'),
      `[Develop ${size.width}×${size.height}] status-line must read "Dry run · … to develop", got "${statusText}"`
    );

    const statusLineBox = await statusLine.boundingBox();
    assert(
      isInsideViewport(statusLineBox, size),
      `[Develop ${size.width}×${size.height}] status-line (${JSON.stringify(statusLineBox)}) not inside viewport`
    );

    // Drawer is open and review-section is inside it, inside the viewport
    const drawer = page.locator('[data-testid="status-details"]');
    assert(
      await drawer.isVisible(),
      `[Develop ${size.width}×${size.height}] Drawer status-details must be open/visible`
    );

    const drawerBox = await drawer.boundingBox();
    assert(
      isInsideViewport(drawerBox, size),
      `[Develop ${size.width}×${size.height}] Drawer (${JSON.stringify(drawerBox)}) not inside viewport`
    );

    const reviewSection = drawer.locator('[data-testid="review-section"]');
    assert(
      (await reviewSection.count()) === 1 && (await reviewSection.isVisible()),
      `[Develop ${size.width}×${size.height}] review-section must be inside drawer and visible`
    );
    const reviewBox = await reviewSection.boundingBox();
    assert(
      reviewBox && reviewBox.y >= 0 && reviewBox.y < size.height,
      `[Develop ${size.width}×${size.height}] review-section (${JSON.stringify(reviewBox)}) not inside viewport`
    );

    // run-button is inside viewport and enabled
    const runBtn = page.locator('[data-testid="run-button"]');
    assert(
      !(await runBtn.isDisabled()),
      `[Develop ${size.width}×${size.height}] run-button must be enabled after dry run`
    );
    const runBox = await runBtn.boundingBox();
    assert(
      isInsideViewport(runBox, size),
      `[Develop ${size.width}×${size.height}] run-button (${JSON.stringify(runBox)}) not inside viewport`
    );

    // .content scrollTop is still 0
    await assertNoScroll(`Develop after dry run ${size.width}×${size.height}`);

    // Screenshot after dry run
    await page.screenshot({ path: join(OUT, `strip-develop-${size.width}.png`) });

    // Click run-button
    await runBtn.click();

    // The strip's line shows the compact progress with a Cancel button inside the viewport
    const cancelBtn = page.locator('[data-testid="status-line"] button').filter({ hasText: 'Cancel' });
    await cancelBtn.waitFor({ state: 'visible', timeout: 5000 });
    const cancelBox = await cancelBtn.boundingBox();
    assert(
      isInsideViewport(cancelBox, size),
      `[Develop ${size.width}×${size.height}] Cancel button (${JSON.stringify(cancelBox)}) not inside viewport`
    );
    await assertNoScroll(`Develop during run ${size.width}×${size.height}`);

    // =========================================================================
    // 2. Escape closes the drawer. The toggle reopens it.
    // =========================================================================
    console.log(`  2. Escape & toggle at ${size.width}×${size.height}...`);
    await page.keyboard.press('Escape');
    await page.waitForTimeout(100);
    assert(
      !(await drawer.isVisible()),
      `[Develop ${size.width}×${size.height}] Escape must close the drawer`
    );

    const toggle = page.locator('[data-testid="status-details-toggle"]');
    assert(
      await toggle.isVisible(),
      `[Develop ${size.width}×${size.height}] Details toggle button must be visible when details exist`
    );
    await toggle.click();
    await page.waitForTimeout(100);
    assert(
      await drawer.isVisible(),
      `[Develop ${size.width}×${size.height}] Details toggle must reopen the drawer`
    );

    // Allow job to complete and settle before switching tabs
    await page.waitForTimeout(600);

    // =========================================================================
    // 3. Switching to Edit and back clears the line and closes the drawer.
    // =========================================================================
    console.log(`  3. Tab switching at ${size.width}×${size.height}...`);
    await page.locator('nav a.nav-item[aria-label="Edit"]').click();
    await page.waitForTimeout(200);

    assert(
      !(await drawer.isVisible()),
      `[Edit ${size.width}×${size.height}] Switching to Edit must close the drawer`
    );

    await page.locator('nav a.nav-item[aria-label="Develop"]').click();
    await page.waitForTimeout(200);

    const devStatusLineCount = await page.locator('[data-testid="status-line"]').count();
    assert(
      devStatusLineCount === 0,
      `[Develop ${size.width}×${size.height}] Switching back to Develop must clear the status line`
    );
    assert(
      !(await drawer.isVisible()),
      `[Develop ${size.width}×${size.height}] Drawer must be closed after returning to Develop`
    );
    await assertNoScroll(`Develop returned ${size.width}×${size.height}`);

    // =========================================================================
    // 4. Batch
    // =========================================================================
    console.log(`  4. Batch at ${size.width}×${size.height}...`);
    await page.locator('nav a.nav-item[aria-label="Batch"]').click();
    await page.waitForTimeout(200);

    const inputsArea = page.locator('textarea');
    await inputsArea.fill('/Volumes/Photos\n/Volumes/Photos/single.jpg');

    const lutSelect = page.locator('select');
    await lutSelect.selectOption({ index: 1 });

    const destInput = page.locator('input[placeholder="/path/to/destination"]');
    await destInput.fill('/Volumes/Photos/graded_output');
    await page.waitForTimeout(100);

    await assertNoScroll(`Batch filled ${size.width}×${size.height}`);

    // Click dry-run-button
    const batchDryRun = page.locator('[data-testid="dry-run-button"]');
    await batchDryRun.click();

    // Wait for review-section
    const batchReviewSection = drawer.locator('[data-testid="review-section"]');
    await batchReviewSection.waitFor({ state: 'visible', timeout: 5000 });

    const batchStatusLine = page.locator('[data-testid="status-line"]');
    await batchStatusLine.waitFor({ state: 'visible', timeout: 5000 });
    const batchStatusText = (await batchStatusLine.textContent()) ?? '';
    assert(
      batchStatusText.includes('Dry run · ') && batchStatusText.includes('to grade'),
      `[Batch ${size.width}×${size.height}] status-line must read "Dry run · … to grade", got "${batchStatusText}"`
    );

    const batchStatusLineBox = await batchStatusLine.boundingBox();
    assert(
      isInsideViewport(batchStatusLineBox, size),
      `[Batch ${size.width}×${size.height}] status-line (${JSON.stringify(batchStatusLineBox)}) not inside viewport`
    );

    // The drawer is open and review-section is inside it, inside the viewport
    assert(
      await drawer.isVisible(),
      `[Batch ${size.width}×${size.height}] Drawer status-details must be open`
    );
    const batchDrawerBox = await drawer.boundingBox();
    assert(
      isInsideViewport(batchDrawerBox, size),
      `[Batch ${size.width}×${size.height}] Drawer (${JSON.stringify(batchDrawerBox)}) not inside viewport`
    );

    const batchReviewBox = await batchReviewSection.boundingBox();
    assert(
      batchReviewBox && batchReviewBox.y >= 0 && batchReviewBox.y < size.height,
      `[Batch ${size.width}×${size.height}] review-section (${JSON.stringify(batchReviewBox)}) not inside viewport`
    );

    // run-button is inside the viewport and enabled
    const batchRunBtn = page.locator('[data-testid="run-button"]');
    assert(
      !(await batchRunBtn.isDisabled()),
      `[Batch ${size.width}×${size.height}] run-button must be enabled after dry run`
    );
    const batchRunBox = await batchRunBtn.boundingBox();
    assert(
      isInsideViewport(batchRunBox, size),
      `[Batch ${size.width}×${size.height}] run-button (${JSON.stringify(batchRunBox)}) not inside viewport`
    );

    // 5. No page or content scroll was needed at any point
    await assertNoScroll(`Batch after dry run ${size.width}×${size.height}`);

    // Screenshot after dry run
    await page.screenshot({ path: join(OUT, `strip-batch-${size.width}.png`) });
  }
} finally {
  await browser.close();
  await server.close();
}

console.log(`\n${passedAssertions} assertions passed`);

if (failures.length > 0) {
  console.error(`\ncheck:strip FAILED with ${failures.length} failure(s):`);
  for (const f of failures) {
    console.error(`  - ${f}`);
  }
  process.exit(1);
} else {
  console.log('check:strip PASSED cleanly.');
}
