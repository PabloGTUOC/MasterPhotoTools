/**
 * RP-1 Develop acceptance & verification script in a real headless browser.
 *
 * Checks (from RP-1.md):
 * 1. Six stubbed RAWs give six thumbnails, and no two thumbnail requests overlap.
 * 2. The picker offers None, 5 Fujifilm, 7 Ricoh and the stubbed LUTs.
 * 3. A look on one selected photograph changes that photograph alone; after Select all it changes all six.
 * 4. The items sent to plan_develop carry the exact builtin:<id> strings and exposures.
 * 5. With a slow first preview response and a fast second, the second stays on screen.
 * 6. Develop is disabled before the dry run and after any change to a look, an exposure or the format, with the reason visible.
 * 7. A stubbed skip is listed with its reason; cancel calls the job cancel; the summary shows its counts.
 * 8. Unmounting and remounting the view keeps folder, assignments and selection.
 * 9. Every control is at least 40 px. Write a screenshot to layout-proof/.
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
const harness = fileURLToPath(new URL('./develop-harness', import.meta.url));
const stubApi = fileURLToPath(new URL('./develop-harness/stub-api.ts', import.meta.url));
const hostSrc = fileURLToPath(new URL('../src', import.meta.url));

console.log('Starting Vite harness server for Develop view...');
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
  viewport: { width: 1280, height: 900 },
  deviceScaleFactor: 2,
});

const failures = [];

try {
  await page.goto(base, { waitUntil: 'networkidle' });
  await page.waitForTimeout(200);

  // -------------------------------------------------------------------------
  // 1. Six stubbed RAWs give six thumbnails, zero overlap
  // -------------------------------------------------------------------------
  console.log('1. Checking folder loading, six thumbnails, and zero overlap in thumbnail requests...');
  const folderInput = page.locator('input[placeholder="/path/to/raw/photos"]');
  await folderInput.fill('/Volumes/Photos/RAW');
  await folderInput.press('Enter');

  // Wait for all 6 thumbnails to load
  await page.waitForSelector('.thumbnail-card:nth-child(6)', { timeout: 3000 });
  // Wait a moment for all thumbnail async requests to finish
  await page.waitForTimeout(300);

  const thumbCount = await page.locator('.thumbnail-card').count();
  if (thumbCount !== 6) {
    failures.push(`Expected 6 thumbnail cards, got ${thumbCount}`);
  }

  const thumbReqs = await page.evaluate(() => window.__DEVELOP_STUB__.thumbnailRequests);
  const maxConcurrentThumbs = await page.evaluate(
    () => window.__DEVELOP_STUB__.maxConcurrentThumbnailRequests,
  );
  console.log(`  Thumbnail requests: ${thumbReqs.length}, max concurrent: ${maxConcurrentThumbs}`);

  if (thumbReqs.length !== 6) {
    failures.push(`Expected 6 thumbnail requests, got ${thumbReqs.length}`);
  }
  if (maxConcurrentThumbs > 1) {
    failures.push(
      `Thumbnail requests must be sequential (zero overlap); max concurrent was ${maxConcurrentThumbs}`,
    );
  }

  // -------------------------------------------------------------------------
  // 2. Look picker groups: None, 5 Fujifilm, 7 Ricoh, stubbed LUTs
  // -------------------------------------------------------------------------
  console.log('2. Checking look picker groups...');
  const lookSelect = page.locator('[data-testid="look-select"]');

  const neutralOptions = await lookSelect.locator('optgroup[label="Neutral"] option').count();
  const fujiOptions = await lookSelect.locator('optgroup[label="Fujifilm"] option').count();
  const ricohOptions = await lookSelect.locator('optgroup[label="Ricoh"] option').count();
  const lutOptions = await lookSelect.locator('optgroup[label="My LUTs"] option').count();

  console.log(
    `  Picker counts: Neutral=${neutralOptions}, Fuji=${fujiOptions}, Ricoh=${ricohOptions}, LUTs=${lutOptions}`,
  );

  if (neutralOptions < 1) {
    failures.push('Picker must offer None (neutral)');
  }
  if (fujiOptions !== 5) {
    failures.push(`Picker must offer exactly 5 Fujifilm looks, got ${fujiOptions}`);
  }
  if (ricohOptions !== 7) {
    failures.push(`Picker must offer exactly 7 Ricoh looks, got ${ricohOptions}`);
  }
  if (lutOptions !== 2) {
    failures.push(`Picker must offer 2 stubbed LUTs, got ${lutOptions}`);
  }

  // -------------------------------------------------------------------------
  // 3. Single photo look assignment vs Select all
  // -------------------------------------------------------------------------
  console.log('3. Checking look assignment to single selection vs Select all...');
  // Photo 1 is currently selected
  await page.selectOption('[data-testid="look-select"]', 'builtin:fujifilm-classic-chrome');
  await page.waitForTimeout(100);

  const card1Look = await page
    .locator('.thumbnail-card[data-filename="DSCF0001.RAF"] .thumb-look')
    .innerText();
  const card2Look = await page
    .locator('.thumbnail-card[data-filename="DSCF0002.RAF"] .thumb-look')
    .innerText();

  if (!card1Look.includes('builtin:fujifilm-classic-chrome')) {
    failures.push(
      `Photo 1 should have builtin:fujifilm-classic-chrome assigned, got: "${card1Look}"`,
    );
  }
  if (!card2Look.includes('None')) {
    failures.push(`Photo 2 should remain unassigned (None), got: "${card2Look}"`);
  }

  // Click Select all and assign Ricoh Positive Film to all
  const selectAllBtn = page.locator('[data-testid="select-all-button"]');
  await selectAllBtn.click();
  await page.waitForTimeout(100);

  await page.selectOption('[data-testid="look-select"]', 'builtin:ricoh-positive-film');
  await page.waitForTimeout(100);

  const allLooks = await page.$$eval('.thumbnail-card .thumb-look', (els) =>
    els.map((el) => el.textContent?.trim()),
  );
  const allRicoh = allLooks.every((l) => l === 'builtin:ricoh-positive-film');
  if (!allRicoh) {
    failures.push(
      `Select all should assign builtin:ricoh-positive-film to all 6 photos; got: ${JSON.stringify(allLooks)}`,
    );
  }

  // -------------------------------------------------------------------------
  // 4. Exact items sent to plan_develop (builtin:<id> strings and exposures)
  // -------------------------------------------------------------------------
  console.log('4. Checking plan_develop items with exact builtin:<id> and exposures...');
  // Select single photo 1 and adjust exposure
  await page.locator('.thumbnail-card[data-filename="DSCF0001.RAF"]').click();
  await page.waitForTimeout(100);

  const exposureInput = page.locator('[data-testid="exposure-slider-number"]');
  await exposureInput.fill('1.5');
  await exposureInput.dispatchEvent('change');
  await page.waitForTimeout(100);

  const dryRunBtn = page.locator('[data-testid="dry-run-button"]');
  await dryRunBtn.click();
  await page.waitForSelector('[data-testid="review-section"]', { timeout: 3000 });

  const plannedItems = await page.evaluate(() => window.__DEVELOP_STUB__.lastPlanItems);
  if (!plannedItems || plannedItems.length !== 6) {
    failures.push(`Expected 6 planned items, got: ${JSON.stringify(plannedItems)}`);
  } else {
    const item1 = plannedItems.find((it) => it.name === 'DSCF0001.RAF');
    const item2 = plannedItems.find((it) => it.name === 'DSCF0002.RAF');

    if (item1?.look !== 'builtin:ricoh-positive-film' || item1?.exposure_ev !== 1.5) {
      failures.push(
        `DSCF0001.RAF should have look="builtin:ricoh-positive-film" and exposure_ev=1.5; got ${JSON.stringify(item1)}`,
      );
    }
    if (item2?.look !== 'builtin:ricoh-positive-film' || item2?.exposure_ev !== 0.0) {
      failures.push(
        `DSCF0002.RAF should have look="builtin:ricoh-positive-film" and exposure_ev=0.0; got ${JSON.stringify(item2)}`,
      );
    }
  }

  // -------------------------------------------------------------------------
  // 5. Stale preview discarded (latest-wins discipline)
  // -------------------------------------------------------------------------
  console.log('5. Checking latest-wins preview rendering...');
  const currentPreviewCalls = await page.evaluate(
    () => window.__DEVELOP_STUB__.previewCallCount,
  );
  // Call 1 (first click): slow response (300ms)
  // Call 2 (second click): fast response (20ms)
  await page.evaluate((c) => {
    window.__DEVELOP_STUB__.previewDelayMs[c + 1] = 300;
    window.__DEVELOP_STUB__.previewDelayMs[c + 2] = 20;
  }, currentPreviewCalls);

  // Click photo 2, then immediately click photo 3
  await page.locator('.thumbnail-card[data-filename="DSCF0002.RAF"]').click();
  await page.locator('.thumbnail-card[data-filename="DSCF0003.RAF"]').click();

  // Wait 400ms for both async preview requests to finish
  await page.waitForTimeout(400);

  const previewTitle = await page.locator('.preview-title').innerText();
  if (!previewTitle.includes('DSCF0003.RAF')) {
    failures.push(
      `Latest preview title should be DSCF0003.RAF, got "${previewTitle}"`,
    );
  }

  // Verify the preview image displayed has the size from call 2, not call 1
  const displayedBlobSize = await page.evaluate(async () => {
    const img = document.querySelector('[data-testid="preview-image"]');
    if (!img || !img.src) return null;
    const res = await fetch(img.src);
    const blob = await res.blob();
    return blob.size;
  });

  const expectedCall2Size = 150 + (currentPreviewCalls + 2) * 10;
  const staleCall1Size = 150 + (currentPreviewCalls + 1) * 10;
  console.log(
    `  Preview blob size: ${displayedBlobSize} (expected second: ${expectedCall2Size}, stale first: ${staleCall1Size})`,
  );
  if (displayedBlobSize !== expectedCall2Size) {
    failures.push(
      `Latest-wins preview failed: expected size ${expectedCall2Size}, got ${displayedBlobSize}`,
    );
  }

  // -------------------------------------------------------------------------
  // 6. Lock discipline: Develop disabled before dry run and after any change
  // -------------------------------------------------------------------------
  console.log('6. Checking lock discipline and gate explanations...');
  const runBtn = page.locator('[data-testid="run-button"]');
  const gateExpl = page.locator('[data-testid="gate-explanation"]');

  // Currently, settings have not been dry run since photo clicks/exposures
  // Let's run a dry run to get into an unlocked state
  await dryRunBtn.click();
  await page.waitForSelector('[data-testid="review-section"]', { timeout: 3000 });

  if (await runBtn.isDisabled()) {
    failures.push('Run button should be enabled immediately after a dry run');
  }

  // 6a. Change look
  console.log('  Testing lock on look change...');
  await page.selectOption('[data-testid="look-select"]', 'builtin:fujifilm-astia');
  await page.waitForTimeout(100);

  if (!(await runBtn.isDisabled())) {
    failures.push('Run button must be disabled after look change');
  }
  const lookChangeReason = await gateExpl.innerText();
  if (!lookChangeReason.includes('Settings have changed')) {
    failures.push(
      `Expected settings changed explanation, got: "${lookChangeReason}"`,
    );
  }

  // Re-run dry run to clear
  await dryRunBtn.click();
  await page.waitForSelector('[data-testid="review-section"]', { timeout: 3000 });
  if (await runBtn.isDisabled()) {
    failures.push('Run button should be re-enabled after dry run');
  }

  // 6b. Change exposure
  console.log('  Testing lock on exposure change...');
  await exposureInput.fill('-1.0');
  await exposureInput.dispatchEvent('change');
  await page.waitForTimeout(100);

  if (!(await runBtn.isDisabled())) {
    failures.push('Run button must be disabled after exposure change');
  }
  const exposureChangeReason = await gateExpl.innerText();
  if (!exposureChangeReason.includes('Settings have changed')) {
    failures.push(
      `Expected settings changed explanation, got: "${exposureChangeReason}"`,
    );
  }

  // Re-run dry run to clear
  await dryRunBtn.click();
  await page.waitForSelector('[data-testid="review-section"]', { timeout: 3000 });

  // 6c. Change format
  console.log('  Testing lock on format change...');
  const tiffRadio = page.locator('[data-testid="format-tiff"]');
  await tiffRadio.click();
  await page.waitForTimeout(100);

  if (!(await runBtn.isDisabled())) {
    failures.push('Run button must be disabled after format change');
  }
  const formatChangeReason = await gateExpl.innerText();
  if (!formatChangeReason.includes('Settings have changed')) {
    failures.push(
      `Expected settings changed explanation, got: "${formatChangeReason}"`,
    );
  }

  // Re-run dry run to unlock for item 7
  await dryRunBtn.click();
  await page.waitForSelector('[data-testid="review-section"]', { timeout: 3000 });
  if (await runBtn.isDisabled()) {
    failures.push('Run button should be enabled after format dry run');
  }

  // -------------------------------------------------------------------------
  // 7. Stubbed skip listed with reason, cancel calls cancel, summary shows counts
  // -------------------------------------------------------------------------
  console.log('7. Checking skipped items, job cancellation, and summary counts...');
  const skippedSection = page.locator('[data-testid="skipped-section"]');
  const skippedText = await skippedSection.innerText();
  console.log(`  Skipped text: "${skippedText}"`);

  if (!skippedText.includes('DSCF0006.RAF') || !skippedText.includes('already developed')) {
    failures.push(`Expected skip listing for DSCF0006.RAF with reason, got: "${skippedText}"`);
  }

  // Click Develop to start job
  await runBtn.click();
  const cancelBtn = page.locator('button.ghost.danger', { hasText: 'Cancel' });
  await cancelBtn.waitFor({ state: 'visible', timeout: 3000 });
  await cancelBtn.click();

  // Wait for cancelled state in JobProgress
  const jobStateSpan = page.locator('.job-state[data-state="cancelled"]');
  await jobStateSpan.waitFor({ state: 'visible', timeout: 3000 });

  const jobMessage = page.locator('.job-message');
  await jobMessage.waitFor({ state: 'visible', timeout: 3000 });
  const summaryMessage = await jobMessage.innerText();
  console.log(`  Summary terminal message: "${summaryMessage}"`);

  if (!summaryMessage.includes('2 developed, 0 failed, 1 skipped')) {
    failures.push(
      `Expected summary counts "2 developed, 0 failed, 1 skipped", got: "${summaryMessage}"`,
    );
  }

  const wasCancelled = await page.evaluate(
    () => window.__DEVELOP_STUB__.cancelledJobIds.length > 0,
  );
  if (!wasCancelled) {
    failures.push('Expected cancelJob to be called on stub client');
  }

  // -------------------------------------------------------------------------
  // 8. Unmounting and remounting keeps folder, assignments, and selection
  // -------------------------------------------------------------------------
  console.log('8. Checking state persistence across view unmount and remount...');
  await page.evaluate(() => window.__unmountDevelop());
  await page.waitForSelector('#unmounted-marker', { timeout: 2000 });

  await page.evaluate(() => window.__remountDevelop());
  await page.waitForSelector('[data-testid="develop-folder-input"]', { timeout: 2000 });
  await page.waitForSelector('.thumbnail-card:nth-child(6)', { timeout: 3000 });

  const remountFolder = await folderInput.inputValue();
  if (remountFolder !== '/Volumes/Photos/RAW') {
    failures.push(`Expected folder "/Volumes/Photos/RAW" preserved, got: "${remountFolder}"`);
  }

  const remountCard1Look = await page
    .locator('.thumbnail-card[data-filename="DSCF0001.RAF"] .thumb-look')
    .innerText();
  if (!remountCard1Look.includes('builtin:ricoh-positive-film')) {
    failures.push(
      `Expected photo 1 assignment "builtin:ricoh-positive-film" preserved across unmount, got: "${remountCard1Look}"`,
    );
  }

  const remountCard3Look = await page
    .locator('.thumbnail-card[data-filename="DSCF0003.RAF"] .thumb-look')
    .innerText();
  if (!remountCard3Look.includes('builtin:fujifilm-astia')) {
    failures.push(
      `Expected photo 3 assignment "builtin:fujifilm-astia" preserved across unmount, got: "${remountCard3Look}"`,
    );
  }

  const isCard3Selected = await page
    .locator('.thumbnail-card[data-filename="DSCF0003.RAF"]')
    .evaluate((el) => el.classList.contains('active'));
  if (!isCard3Selected) {
    failures.push('Expected selection of DSCF0003.RAF preserved across unmount');
  }

  // -------------------------------------------------------------------------
  // 9. Interactive touch targets >= 40px and layout proof screenshot
  // -------------------------------------------------------------------------
  console.log('9. Auditing touch target heights (>= 40px) and taking screenshot...');
  const interactiveHandles = await page.$$(
    'button, input[type="text"], input[type="number"], textarea, select, label.format-option',
  );

  let under40Count = 0;
  for (const handle of interactiveHandles) {
    const isVisible = await handle.isVisible();
    if (!isVisible) continue;
    const box = await handle.boundingBox();
    if (box && box.height < 39.5) {
      const tag = await handle.evaluate(
        (el) => `${el.tagName.toLowerCase()}${el.className ? '.' + el.className : ''}`,
      );
      console.warn(`Control under 40px: <${tag}> height=${box.height}px`);
      under40Count++;
    }
  }
  if (under40Count > 0) {
    failures.push(`Found ${under40Count} interactive control(s) with height under 40px`);
  }

  const screenshotPath = join(OUT, 'develop.png');
  await page.screenshot({ path: screenshotPath, fullPage: true });
  console.log(`Wrote layout screenshot proof to ${screenshotPath}`);

} catch (err) {
  failures.push(`Test execution threw: ${err instanceof Error ? err.stack : String(err)}`);
} finally {
  await browser.close();
  await server.close();
}

if (failures.length > 0) {
  console.error('\ncheck:develop FAILED:');
  for (const f of failures) {
    console.error(`- ${f}`);
  }
  process.exit(1);
} else {
  console.log('\ncheck:develop PASSED: all 9 checks verified successfully.');
}
