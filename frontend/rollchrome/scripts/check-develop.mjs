/**
 * RP-1 Develop acceptance & verification script in a real headless browser.
 *
 * Checks (from RP-1.md and Review Round 1):
 * 1. Six stubbed RAWs give six thumbnails, zero overlap in thumbnail requests.
 * 2. The picker offers None, 5 Fujifilm, 7 Ricoh, and stubbed LUTs (using the 12 shipped looks).
 * 3. A look on one selected photograph changes that photograph alone; after Select all it changes all six.
 *    Thumbnails show the display name ("Positive Film"), not internal id.
 * 4. The items sent to plan_develop carry the exact builtin:<id> strings and exposures.
 * 5. With a slow first preview response and a fast second, the second stays on screen (assert naturalWidth).
 * 6. Develop is disabled before the dry run and after any change to a look, an exposure or the format, with the reason visible.
 * 7. A stubbed skip is listed with its reason; cancel calls the job cancel; the summary shows its counts.
 * 8. Unmounting and remounting the view keeps folder, assignments, and selection.
 * 9. Every control is at least 40 px. Write a screenshot to layout-proof/develop.png. Assert naturalWidth > 0 on thumbnails and preview.
 * 10. During a burst of ten exposure changes against a slow stub, no two preview requests overlap, at most two are sent, and the last carries the final exposure.
 * 11. Assigning a look, then changing to a folder with a file of the same name, shows that file with no look.
 * 12. Unmounting and remounting while thumbnails load gives no overlapping thumbnail requests.
 * 13. Remounting during a run shows its progress and keeps Develop disabled.
 * 14. With Selected only, the items sent to plan_develop are exactly the selection, and changing the selection locks Develop.
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
  // 2. Look picker groups: None, 5 Fujifilm, 7 Ricoh, stubbed LUTs (12 shipped looks)
  // -------------------------------------------------------------------------
  console.log('2. Checking look picker groups (12 shipped looks)...');
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
  // 3. Single photo look assignment vs Select all; Display name on thumbnails
  // -------------------------------------------------------------------------
  console.log('3. Checking look assignment to single selection vs Select all and display names...');
  // Photo 1 is currently selected
  await page.selectOption('[data-testid="look-select"]', 'builtin:fujifilm-classic-chrome');
  await page.waitForTimeout(100);

  const card1Look = await page
    .locator('.thumbnail-card[data-filename="DSCF0001.RAF"] .thumb-look')
    .innerText();
  const card2Look = await page
    .locator('.thumbnail-card[data-filename="DSCF0002.RAF"] .thumb-look')
    .innerText();

  if (card1Look !== 'Classic Chrome') {
    failures.push(
      `Photo 1 should show display name "Classic Chrome", got: "${card1Look}"`,
    );
  }
  if (card2Look !== 'None') {
    failures.push(`Photo 2 should show display name "None", got: "${card2Look}"`);
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
  const allRicoh = allLooks.every((l) => l === 'Positive Film');
  if (!allRicoh) {
    failures.push(
      `Select all should assign "Positive Film" display name to all 6 photos; got: ${JSON.stringify(allLooks)}`,
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
  // Call 1: slow response (250ms), returns 64x64 JPEG
  // Call 2: fast response (20ms), returns 80x64 JPEG
  await page.evaluate((c) => {
    window.__DEVELOP_STUB__.previewDelayMs[c + 1] = 250;
    window.__DEVELOP_STUB__.previewDelayMs[c + 2] = 20;
    window.__DEVELOP_STUB__.distinctPreviewCallIdx = c + 2;
  }, currentPreviewCalls);

  // Click photo 2, then immediately click photo 3
  await page.locator('.thumbnail-card[data-filename="DSCF0002.RAF"]').click();
  await page.locator('.thumbnail-card[data-filename="DSCF0003.RAF"]').click();

  // Wait for serialized calls to finish
  await page.waitForTimeout(400);

  const previewTitle = await page.locator('.preview-title').innerText();
  if (!previewTitle.includes('DSCF0003.RAF')) {
    failures.push(
      `Latest preview title should be DSCF0003.RAF, got "${previewTitle}"`,
    );
  }

  // Verify the preview image displayed has width 80 (from call 2), not 64 (from call 1)
  const displayedNaturalWidth = await page.evaluate(() => {
    const img = document.querySelector('[data-testid="preview-image"]');
    return img ? img.naturalWidth : 0;
  });

  console.log(
    `  Preview naturalWidth: ${displayedNaturalWidth} (expected second call: 80, stale first: 64)`,
  );
  if (displayedNaturalWidth !== 80) {
    failures.push(
      `Latest-wins preview failed: expected naturalWidth 80, got ${displayedNaturalWidth}`,
    );
  }

  // -------------------------------------------------------------------------
  // 6. Lock discipline: Develop disabled before dry run and after any change
  // -------------------------------------------------------------------------
  console.log('6. Checking lock discipline and gate explanations...');
  const runBtn = page.locator('[data-testid="run-button"]');
  const gateExpl = page.locator('[data-testid="gate-explanation"]');

  await dryRunBtn.click();
  await page.waitForSelector('[data-testid="review-section"]', { timeout: 3000 });

  if (await runBtn.isDisabled()) {
    failures.push('Run button should be enabled immediately after a dry run');
  }

  // 6a. Change look
  console.log('  Testing lock on look change...');
  await page.selectOption('[data-testid="look-select"]', 'builtin:fujifilm-provia');
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
  await page.waitForFunction(() => document.querySelectorAll('.thumb-img').length === 6, {
    timeout: 5000,
  });

  const remountFolder = await folderInput.inputValue();
  if (remountFolder !== '/Volumes/Photos/RAW') {
    failures.push(`Expected folder "/Volumes/Photos/RAW" preserved, got: "${remountFolder}"`);
  }

  const remountCard1Look = await page
    .locator('.thumbnail-card[data-filename="DSCF0001.RAF"] .thumb-look')
    .innerText();
  if (!remountCard1Look.includes('Positive Film')) {
    failures.push(
      `Expected photo 1 assignment "Positive Film" preserved across unmount, got: "${remountCard1Look}"`,
    );
  }

  const isCard3Selected = await page
    .locator('.thumbnail-card[data-filename="DSCF0003.RAF"]')
    .evaluate((el) => el.classList.contains('active'));
  if (!isCard3Selected) {
    failures.push('Expected selection of DSCF0003.RAF preserved across unmount');
  }

  // -------------------------------------------------------------------------
  // 9. Interactive touch targets >= 40px and naturalWidth > 0 image proof
  // -------------------------------------------------------------------------
  console.log('9. Auditing touch target heights (>= 40px) and image naturalWidth...');
  const interactiveHandles = await page.$$(
    'button, input[type="text"], input[type="number"], textarea, select, label.format-option, label.scope-option',
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

  // Assert naturalWidth > 0 on all visible thumbnails and preview image (RP-1 review item 7)
  const imageMetrics = await page.evaluate(() => {
    const thumbImgs = Array.from(document.querySelectorAll('.thumb-img')).map(
      (el) => el.naturalWidth,
    );
    const prev = document.querySelector('[data-testid="preview-image"]');
    return {
      thumbnails: thumbImgs,
      preview: prev ? prev.naturalWidth : 0,
    };
  });
  console.log(`  Thumbnail naturalWidths: ${JSON.stringify(imageMetrics.thumbnails)}, preview: ${imageMetrics.preview}`);

  if (imageMetrics.thumbnails.length !== 6 || imageMetrics.thumbnails.some((w) => w <= 0)) {
    failures.push(
      `Every thumbnail must render a valid image with naturalWidth > 0, got: ${JSON.stringify(imageMetrics.thumbnails)}`,
    );
  }
  if (imageMetrics.preview <= 0) {
    failures.push(`Preview image must have naturalWidth > 0, got: ${imageMetrics.preview}`);
  }

  const screenshotPath = join(OUT, 'develop.png');
  await page.screenshot({ path: screenshotPath, fullPage: true });
  console.log(`Wrote layout screenshot proof to ${screenshotPath}`);

  // -------------------------------------------------------------------------
  // 10. Exposure scrub burst serialization (RP-1 review item 1)
  // -------------------------------------------------------------------------
  console.log('10. Checking serialized preview requests during exposure slider scrub...');
  await page.evaluate(() => {
    window.__DEVELOP_STUB__.previewRequests = [];
    window.__DEVELOP_STUB__.previewCallCount = 0;
    window.__DEVELOP_STUB__.maxConcurrentPreviewRequests = 0;
    window.__DEVELOP_STUB__.previewDelayMs = { 1: 100, 2: 100 };
  });

  // Photo 3 is selected; burst of 10 exposure changes
  for (let i = 1; i <= 10; i++) {
    const val = (i * 0.2).toFixed(1);
    await exposureInput.fill(val);
    await exposureInput.dispatchEvent('change');
  }

  // Wait for in-flight and pending preview requests to settle
  await page.waitForTimeout(350);

  const burstRequests = await page.evaluate(() => window.__DEVELOP_STUB__.previewRequests);
  const burstMaxConcurrent = await page.evaluate(
    () => window.__DEVELOP_STUB__.maxConcurrentPreviewRequests,
  );
  console.log(
    `  Burst preview requests: ${burstRequests.length}, max concurrent: ${burstMaxConcurrent}`,
  );

  if (burstMaxConcurrent > 1) {
    failures.push(
      `Preview requests during burst must not overlap; max concurrent was ${burstMaxConcurrent}`,
    );
  }
  if (burstRequests.length > 2) {
    failures.push(
      `During burst of 10 exposure changes, at most 2 preview requests should be sent, got ${burstRequests.length}`,
    );
  }
  const lastBurstReq = burstRequests[burstRequests.length - 1];
  if (!lastBurstReq || Math.abs(lastBurstReq.exposure_ev - 2.0) > 0.01) {
    failures.push(
      `Last preview request should carry final exposure 2.0, got: ${JSON.stringify(lastBurstReq)}`,
    );
  }

  // -------------------------------------------------------------------------
  // 11. Folder change clears assignments (RP-1 review item 2)
  // -------------------------------------------------------------------------
  console.log('11. Checking assignments cleared on folder change...');
  await folderInput.fill('/Volumes/Photos/folder2');
  await folderInput.press('Enter');
  await page.waitForSelector('.thumbnail-card:nth-child(2)', { timeout: 3000 });
  await page.waitForTimeout(200);

  const folder2Card1Look = await page
    .locator('.thumbnail-card[data-filename="DSCF0001.RAF"] .thumb-look')
    .innerText();
  if (folder2Card1Look !== 'None') {
    failures.push(
      `Changing to folder with same-named file should show "None", got "${folder2Card1Look}"`,
    );
  }

  // Switch back to /Volumes/Photos/RAW
  await folderInput.fill('/Volumes/Photos/RAW');
  await folderInput.press('Enter');
  await page.waitForSelector('.thumbnail-card:nth-child(6)', { timeout: 3000 });
  await page.waitForTimeout(200);

  // -------------------------------------------------------------------------
  // 12. Unmount/remount while thumbnails load gives no overlap (RP-1 review item 3)
  // -------------------------------------------------------------------------
  console.log('12. Checking unmount and remount during thumbnail loading...');
  await page.evaluate(() => {
    window.__DEVELOP_STUB__.thumbnailDelayMs = 150;
    window.__DEVELOP_STUB__.maxConcurrentThumbnailRequests = 0;
    window.__DEVELOP_STUB__.thumbnailRequests = [];
  });

  // Trigger load by re-entering folder
  await folderInput.fill('/Volumes/Photos/RAW');
  await folderInput.press('Enter');
  await page.waitForTimeout(30);

  // Unmount while loading
  await page.evaluate(() => window.__unmountDevelop());
  await page.waitForSelector('#unmounted-marker', { timeout: 2000 });
  await page.waitForTimeout(50);

  // Remount
  await page.evaluate(() => window.__remountDevelop());
  await page.waitForSelector('.thumbnail-card:nth-child(6)', { timeout: 4000 });
  await page.waitForTimeout(200);

  const unmountMaxConcurrentThumbs = await page.evaluate(
    () => window.__DEVELOP_STUB__.maxConcurrentThumbnailRequests,
  );
  if (unmountMaxConcurrentThumbs > 1) {
    failures.push(
      `Unmounting/remounting while thumbnails load must not cause overlapping requests; max was ${unmountMaxConcurrentThumbs}`,
    );
  }
  await page.evaluate(() => {
    window.__DEVELOP_STUB__.thumbnailDelayMs = 20;
  });

  // -------------------------------------------------------------------------
  // 13. Remounting during a run shows progress & keeps Develop disabled (RP-1 review item 4)
  // -------------------------------------------------------------------------
  console.log('13. Checking running job persistence across remount...');
  await dryRunBtn.click();
  await page.waitForSelector('[data-testid="review-section"]', { timeout: 3000 });

  await runBtn.click();
  await page.waitForSelector('[data-testid="job-section"]', { timeout: 3000 });

  // Unmount while job is running
  await page.evaluate(() => window.__unmountDevelop());
  await page.waitForSelector('#unmounted-marker', { timeout: 2000 });

  // Remount
  await page.evaluate(() => window.__remountDevelop());
  await page.waitForSelector('[data-testid="develop-folder-input"]', { timeout: 2000 });

  const jobSectionVisible = await page.locator('[data-testid="job-section"]').isVisible();
  if (!jobSectionVisible) {
    failures.push('JobProgress section must remain visible on remount during a running job');
  }
  const isDevelopDisabledDuringRun = await runBtn.isDisabled();
  if (!isDevelopDisabledDuringRun) {
    failures.push('Develop button must remain disabled during a running job after remount');
  }

  // Cancel job to settle
  const cancelBtnDuringRun = page.locator('button.ghost.danger', { hasText: 'Cancel' });
  if (await cancelBtnDuringRun.isVisible()) {
    await cancelBtnDuringRun.click();
  }
  await page.waitForTimeout(200);

  // -------------------------------------------------------------------------
  // 14. Scope: Selected only sends selection and locks on selection change (RP-1 review item 5)
  // -------------------------------------------------------------------------
  console.log('14. Checking scope toggle: Selected only items and lock on selection change...');
  const scopeSelected = page.locator('[data-testid="scope-selected"]');
  await scopeSelected.click();
  await page.waitForTimeout(100);

  // Select photo 1 and photo 2
  await page.locator('.thumbnail-card[data-filename="DSCF0001.RAF"]').click();
  await page.locator('.thumbnail-card[data-filename="DSCF0002.RAF"]').click({ modifiers: ['Meta'] });
  await page.waitForTimeout(100);

  // Dry run with Selected only
  await dryRunBtn.click();
  await page.waitForSelector('[data-testid="review-section"]', { timeout: 3000 });

  const selectedPlannedItems = await page.evaluate(
    () => window.__DEVELOP_STUB__.lastPlanItems,
  );
  if (!selectedPlannedItems || selectedPlannedItems.length !== 2) {
    failures.push(
      `With Selected only, expected 2 planned items, got: ${JSON.stringify(selectedPlannedItems)}`,
    );
  } else {
    const names = selectedPlannedItems.map((it) => it.name).sort();
    if (names[0] !== 'DSCF0001.RAF' || names[1] !== 'DSCF0002.RAF') {
      failures.push(
        `With Selected only, expected [DSCF0001.RAF, DSCF0002.RAF], got: ${JSON.stringify(names)}`,
      );
    }
  }

  if (await runBtn.isDisabled()) {
    failures.push('Run button should be enabled after dry run with Selected only');
  }

  // Changing selection must lock Develop
  await page.locator('.thumbnail-card[data-filename="DSCF0003.RAF"]').click();
  await page.waitForTimeout(100);

  if (!(await runBtn.isDisabled())) {
    failures.push('Changing selection under Selected only must lock Develop');
  }
  const selectionChangeReason = await gateExpl.innerText();
  if (!selectionChangeReason.includes('Settings have changed')) {
    failures.push(
      `Expected settings changed explanation when selection changes under Selected only, got: "${selectionChangeReason}"`,
    );
  }

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
  console.log('\ncheck:develop PASSED: all 14 checks verified successfully.');
}
