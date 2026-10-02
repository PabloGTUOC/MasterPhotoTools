/**
 * ED-8 Bulk LUT acceptance & verification script in a real headless browser.
 *
 * Checks (from ed8-brief.md):
 * - Run is disabled before a dry run, and again after any setting changes, with the reason visible.
 * - The strip shows as many frames as sample_frames returned, and the previews are requested
 *   sequentially (no two preview sessions at once).
 * - The run sends the reviewed lut_sha256, and a stubbed 'LUT changed' refusal is shown verbatim.
 * - Cancel calls the job cancel, and the summary shows written, skipped and failed counts.
 * - Every control is at least 40 px; screenshot to layout-proof/.
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
const harness = fileURLToPath(new URL('./bulk-lut-harness', import.meta.url));
const stubApi = fileURLToPath(new URL('./bulk-lut-harness/stub-api.ts', import.meta.url));
const hostSrc = fileURLToPath(new URL('../src', import.meta.url));

console.log('Starting Vite harness server for Bulk LUT view...');
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
  // 1. Initial lock state & gate explanation
  // -------------------------------------------------------------------------
  console.log('Verifying initial locked state before dry run...');
  const runBtn = page.locator('[data-testid="run-button"]');
  const dryRunBtn = page.locator('[data-testid="dry-run-button"]');
  const gateExpl = page.locator('[data-testid="gate-explanation"]');

  if (!(await runBtn.isDisabled())) {
    failures.push('Run button must be disabled before any settings or dry run');
  }

  const initialGateText = await gateExpl.innerText();
  if (!initialGateText.includes('Run is unavailable until a dry run') || !initialGateText.includes('No input files or folders selected')) {
    failures.push(`Expected gate explanation to detail missing inputs and missing dry run, got: "${initialGateText}"`);
  }

  // -------------------------------------------------------------------------
  // 2. Fill settings
  // -------------------------------------------------------------------------
  console.log('Filling inputs and selecting LUT...');
  const inputsArea = page.locator('textarea');
  await inputsArea.fill('/Volumes/Photos\n/Volumes/Photos/single.jpg');

  // Select a LUT from the LutPicker dropdown
  const lutSelect = page.locator('select');
  await lutSelect.selectOption({ index: 1 });

  // Fill destination folder
  const destInput = page.locator('input[placeholder="/path/to/destination"]');
  await destInput.fill('/Volumes/Photos/graded_output');

  await page.waitForTimeout(100);

  // Run button should still be disabled because dry run hasn't run yet
  if (!(await runBtn.isDisabled())) {
    failures.push('Run button must remain disabled before dry run is executed');
  }

  const preDryRunGate = await gateExpl.innerText();
  if (!preDryRunGate.includes('Run is unavailable until a dry run of these settings has been reviewed.')) {
    failures.push(`Gate explanation should say run is unavailable until dry run, got: "${preDryRunGate}"`);
  }

  // -------------------------------------------------------------------------
  // 3. Dry run execution & sequential sample preview strip
  // -------------------------------------------------------------------------
  console.log('Triggering dry run...');
  await dryRunBtn.click();

  // Wait for review section, sample strip, and all sample previews to complete rendering
  const reviewSection = page.locator('[data-testid="review-section"]');
  await reviewSection.waitFor({ state: 'visible', timeout: 5000 });

  const sampleFrames = page.locator('[data-testid="sample-frame"]');
  await page.waitForFunction(() => {
    return document.querySelectorAll('[data-testid="sample-frame"]').length === 3;
  }, { timeout: 5000 });

  // Wait until planning is completely finished (isBusy is false)
  await page.waitForFunction(() => {
    const btn = document.querySelector('[data-testid="dry-run-button"]');
    return btn && !btn.textContent.includes('Planning');
  }, { timeout: 5000 });

  const frameCount = await sampleFrames.count();
  console.log(`Rendered ${frameCount} sample frame cards in preview strip.`);
  if (frameCount !== 3) {
    failures.push(`Expected 3 sample frames, found ${frameCount}`);
  }

  // Verify sequential preview sessions
  const stubMetrics = await page.evaluate(() => window.__BULK_LUT_STUB__);
  console.log(`Stub metrics: maxConcurrentSessions=${stubMetrics.maxConcurrentSessions}, samplePreviewsRequested=${stubMetrics.samplePreviewsRequested.length}`);
  if (stubMetrics.maxConcurrentSessions > 1) {
    failures.push(`Preview sessions must be sequential (max 1 concurrent session), but saw maxConcurrentSessions=${stubMetrics.maxConcurrentSessions}`);
  }
  if (stubMetrics.samplePreviewsRequested.length !== 3) {
    failures.push(`Expected 3 sample preview requests, got ${stubMetrics.samplePreviewsRequested.length}`);
  }

  // Run button should now be enabled
  if (await runBtn.isDisabled()) {
    failures.push('Run button must be enabled after dry run completes');
  }
  if (await gateExpl.isVisible()) {
    failures.push('Gate explanation must be hidden when all conditions are satisfied');
  }

  // -------------------------------------------------------------------------
  // 3b. Sample preview failure keeps entry with error in strip (G10)
  // -------------------------------------------------------------------------
  console.log('Testing sample preview failure handling in preview strip...');
  await page.evaluate(() => {
    window.__BULK_LUT_STUB__.failSamplePreviewPath = 'frame2.jpg';
  });
  await dryRunBtn.click();
  await page.waitForFunction(() => {
    const btn = document.querySelector('[data-testid="dry-run-button"]');
    return btn && !btn.textContent.includes('Planning');
  }, { timeout: 5000 });

  const sampleFramesAfterErr = page.locator('[data-testid="sample-frame"]');
  const countAfterErr = await sampleFramesAfterErr.count();
  if (countAfterErr !== 3) {
    failures.push(`Expected 3 sample frames even when one fails to preview, got ${countAfterErr}`);
  }

  const sampleErrorDisplay = page.locator('[data-testid="sample-error"]');
  await sampleErrorDisplay.waitFor({ state: 'visible', timeout: 3000 });
  const sampleErrorText = await sampleErrorDisplay.innerText();
  console.log(`Sample preview error card text: "${sampleErrorText}"`);
  if (!sampleErrorText.includes("Couldn't preview frame2.jpg: Corrupted preview for /Volumes/Photos/frame2.jpg")) {
    failures.push(`Expected error card to say "Couldn't preview frame2.jpg: Corrupted preview...", got: "${sampleErrorText}"`);
  }

  // Clear simulated preview failure
  await page.evaluate(() => {
    window.__BULK_LUT_STUB__.failSamplePreviewPath = null;
  });

  // -------------------------------------------------------------------------
  // 4. Setting change invalidates dry run with reason
  // -------------------------------------------------------------------------
  console.log('Testing dry-run invalidation on setting change...');
  const recursiveCheckbox = page.locator('input[type="checkbox"]');
  await recursiveCheckbox.check();
  await page.waitForTimeout(100);

  if (!(await runBtn.isDisabled())) {
    failures.push('Run button must be disabled after settings change');
  }

  const invalidatedGate = await gateExpl.innerText();
  if (!invalidatedGate.includes('Settings have changed since the dry run — run it again so the review matches what will be graded.')) {
    failures.push(`Expected "Settings have changed since the dry run" reason, got: "${invalidatedGate}"`);
  }

  // Re-run dry run to re-enable
  console.log('Re-running dry run after settings change...');
  await dryRunBtn.click();
  await page.waitForFunction(() => {
    const btn = document.querySelector('[data-testid="dry-run-button"]');
    return btn && !btn.textContent.includes('Planning');
  }, { timeout: 5000 });
  await page.waitForTimeout(100);

  if (await runBtn.isDisabled()) {
    failures.push('Run button must become enabled again after re-running dry run');
  }

  // -------------------------------------------------------------------------
  // 5. Run passes reviewed lut_sha256, and stubbed "LUT changed" refusal is shown verbatim
  // -------------------------------------------------------------------------
  console.log('Testing core refusal when LUT changed on disk...');
  await page.evaluate(() => {
    window.__BULK_LUT_STUB__.failNextApplyWithLutChanged = true;
  });

  await runBtn.click();
  await page.waitForTimeout(200);

  const refusalMsg = page.locator('[data-testid="refusal-message"]');
  await refusalMsg.waitFor({ state: 'visible', timeout: 3000 });
  const refusalText = await refusalMsg.innerText();
  console.log(`Core refusal text: "${refusalText}"`);

  const expectedRefusal = 'The LUT file changed on disk since the reviewed plan (expected 7f83b1657ff1fc53b92dc18148a1d65dfc2d4b1fa3d677284addd200126d9069, found e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855); please run another dry run';
  if (!refusalText.includes(expectedRefusal)) {
    failures.push(`Expected verbatim LUT changed refusal, got: "${refusalText}"`);
  }

  const applyHash = await page.evaluate(() => window.__BULK_LUT_STUB__.lastApplyLutSha256);
  if (applyHash !== stubMetrics.lastPlanLutSha256) {
    failures.push(`Expected run to pass reviewed hash ${stubMetrics.lastPlanLutSha256}, got ${applyHash}`);
  }

  // -------------------------------------------------------------------------
  // 6. Cancellation tests
  // -------------------------------------------------------------------------
  console.log('Testing cancelJob error handling when cancel throws...');
  await page.evaluate(() => {
    window.__BULK_LUT_STUB__.failNextApplyWithLutChanged = false;
    window.__BULK_LUT_STUB__.failNextCancelWithThrow = true;
  });

  await runBtn.click();
  const cancelBtn1 = page.locator('button.ghost.danger', { hasText: 'Cancel' });
  await cancelBtn1.waitFor({ state: 'visible', timeout: 3000 });
  await cancelBtn1.click();

  const cancelThrowRefusal = page.locator('[data-testid="refusal-message"]');
  await cancelThrowRefusal.waitFor({ state: 'visible', timeout: 3000 });
  const cancelThrowText = await cancelThrowRefusal.innerText();
  console.log(`Cancel throw refusal text: "${cancelThrowText}"`);
  if (!cancelThrowText.includes('Failed to cancel job: IPC cancel error')) {
    failures.push(`Expected "Failed to cancel job: IPC cancel error", got: "${cancelThrowText}"`);
  }

  // Wait for running job to finish
  await page.waitForTimeout(600);

  console.log('Testing cancelJob error handling when cancel returns false...');
  await page.evaluate(() => {
    window.__BULK_LUT_STUB__.failNextCancelWithFalse = true;
  });

  await runBtn.click();
  const cancelBtn2 = page.locator('button.ghost.danger', { hasText: 'Cancel' });
  await cancelBtn2.waitFor({ state: 'visible', timeout: 3000 });
  await cancelBtn2.click();

  const cancelFalseRefusal = page.locator('[data-testid="refusal-message"]');
  await cancelFalseRefusal.waitFor({ state: 'visible', timeout: 3000 });
  const cancelFalseText = await cancelFalseRefusal.innerText();
  console.log(`Cancel false refusal text: "${cancelFalseText}"`);
  if (!cancelFalseText.includes('The job had already finished.')) {
    failures.push(`Expected "The job had already finished.", got: "${cancelFalseText}"`);
  }

  // Wait for running job to finish
  await page.waitForTimeout(600);

  console.log('Testing successful job cancellation...');
  await runBtn.click();
  const cancelBtn3 = page.locator('button.ghost.danger', { hasText: 'Cancel' });
  await cancelBtn3.waitFor({ state: 'visible', timeout: 3000 });
  await cancelBtn3.click();

  // Wait for cancelled state and summary
  const jobStateSpan = page.locator('.job-state[data-state="cancelled"]');
  await jobStateSpan.waitFor({ state: 'visible', timeout: 3000 });

  const jobMessage = page.locator('.job-message');
  await jobMessage.waitFor({ state: 'visible', timeout: 3000 });
  const messageText = await jobMessage.innerText();
  console.log(`Job terminal message after cancel: "${messageText}"`);

  if (!messageText.includes('2 graded, 0 failed, 1 skipped')) {
    failures.push(`Expected cancelled job summary "2 graded, 0 failed, 1 skipped", got: "${messageText}"`);
  }

  const wasCancelled = await page.evaluate(() => window.__BULK_LUT_STUB__.cancelledJobIds.length > 0);
  if (!wasCancelled) {
    failures.push('Expected cancelJob to be called on stub client');
  }

  // -------------------------------------------------------------------------
  // 6b. Batch from a preset (ED-18): same lock discipline, different source
  // -------------------------------------------------------------------------
  console.log('Testing a batch graded with a preset (ED-18)...');
  await page.waitForTimeout(600); // let the cancelled job settle
  const waitPlanned = () =>
    page.waitForFunction(() => {
      const btn = document.querySelector('[data-testid="dry-run-button"]');
      return btn && !btn.textContent.includes('Planning');
    }, { timeout: 5000 });

  await page.locator('[data-testid="source-preset"]').click();
  await page.waitForTimeout(50);
  const presetGate = await gateExpl.innerText();
  if (!(await runBtn.isDisabled()) || !presetGate.includes('No preset selected.') || !presetGate.includes('Settings have changed since the dry run')) {
    failures.push(`Switching to Preset must lock the run and say why; gate: "${presetGate}"`);
  }
  if (await page.locator('[data-testid="batch-preset-select"]').count() !== 1) {
    failures.push('Preset source must show the preset picker');
  }

  const rendersBefore = await page.evaluate(() => window.__BULK_LUT_STUB__.renderedRecipes.length);
  await page.selectOption('[data-testid="batch-preset-select"]', 'Warm Film');
  await dryRunBtn.click();
  await waitPlanned();
  await page.waitForTimeout(100);

  const presetReview = await page.evaluate((before) => ({
    planSource: window.__BULK_LUT_STUB__.lastPlanEditSource,
    rendered: window.__BULK_LUT_STUB__.renderedRecipes.slice(before),
    reviewText: document.querySelector('[data-testid="review-preset"]')?.textContent ?? '',
    naming: document.querySelector('.rule-box code')?.textContent ?? '',
    frames: document.querySelectorAll('[data-testid="sample-frame"]').length,
  }), rendersBefore);
  if (JSON.stringify(presetReview.planSource) !== JSON.stringify({ kind: 'Preset', name: 'Warm Film' })) {
    failures.push(`Dry run should plan the preset by name; sent ${JSON.stringify(presetReview.planSource)}`);
  }
  if (presetReview.frames !== 3 || presetReview.rendered.length !== 3 || presetReview.rendered.some((r) => r.exposure !== 0.4 || r.temperature !== 12)) {
    failures.push(`Sample frames should be rendered with the whole preset; got ${JSON.stringify(presetReview)}`);
  }
  if (!presetReview.reviewText.includes('Warm Film') || !presetReview.naming.includes('_edit')) {
    failures.push(`Review should name the preset and the _edit suffix; got ${JSON.stringify(presetReview)}`);
  }
  if (await runBtn.isDisabled()) failures.push('Run must unlock after a preset dry run');

  // Choosing another preset locks the run again.
  await page.selectOption('[data-testid="batch-preset-select"]', 'Cool Matte');
  await page.waitForTimeout(50);
  if (!(await runBtn.isDisabled())) failures.push('Changing the preset after the dry run must lock the run');
  await page.selectOption('[data-testid="batch-preset-select"]', 'Warm Film');
  await page.waitForTimeout(50);

  // A preset replaced after the review: core's refusal is shown verbatim.
  await page.evaluate(() => {
    window.__BULK_LUT_STUB__.failNextApplyWithRecipeChanged = true;
  });
  await runBtn.click();
  await page.waitForTimeout(150);
  const staleText = await page.locator('[data-testid="refusal-message"]').innerText();
  if (!staleText.includes('The recipe changed since the dry run (the preset was edited or replaced); run another dry run')) {
    failures.push(`Expected the stale-preset refusal verbatim, got "${staleText}"`);
  }

  await runBtn.click();
  await page.waitForTimeout(150);
  const applied = await page.evaluate(() => window.__BULK_LUT_STUB__.lastApplyEdit);
  const plannedSha = await page.evaluate(() => window.__BULK_LUT_STUB__.lastPlanEditSha);
  if (
    !applied ||
    JSON.stringify(applied.source) !== JSON.stringify({ kind: 'Preset', name: 'Warm Film' }) ||
    applied.reviewedRecipeSha256 !== plannedSha
  ) {
    failures.push(`Run should send the reviewed preset and its recipe hash; sent ${JSON.stringify(applied)}`);
  } else {
    console.log('  Preset batch: plans by name, previews the whole look, locks on change, sends the reviewed hash, shows refusals.');
  }
  await page.waitForTimeout(600);
  await page.screenshot({ path: join(OUT, 'batch-preset.png'), fullPage: true });

  // -------------------------------------------------------------------------
  // 7. Interactive touch targets >= 40px
  // -------------------------------------------------------------------------
  console.log('Auditing touch target heights (>= 40px)...');
  const interactiveHandles = await page.$$(
    'button, input[type="text"], textarea, select, label.checkbox',
  );

  let under40Count = 0;
  for (const handle of interactiveHandles) {
    const isVisible = await handle.isVisible();
    if (!isVisible) continue;
    const box = await handle.boundingBox();
    if (box && box.height < 39.5) {
      const tag = await handle.evaluate((el) => `${el.tagName.toLowerCase()}${el.className ? '.' + el.className : ''}`);
      console.warn(`Control under 40px: <${tag}> height=${box.height}px`);
      under40Count++;
    }
  }
  if (under40Count > 0) {
    failures.push(`Found ${under40Count} interactive control(s) with height under 40px`);
  }

  // Screenshot proof
  const screenshotPath = join(OUT, 'bulk-lut.png');
  await page.screenshot({ path: screenshotPath, fullPage: true });
  console.log(`Wrote layout screenshot proof to ${screenshotPath}`);

} catch (err) {
  failures.push(`Test execution threw: ${err instanceof Error ? err.stack : String(err)}`);
} finally {
  await browser.close();
  await server.close();
}

if (failures.length > 0) {
  console.error('\ncheck:bulk-lut FAILED:');
  for (const f of failures) {
    console.error(`- ${f}`);
  }
  process.exit(1);
} else {
  console.log('\ncheck:bulk-lut PASSED cleanly: all acceptance assertions and benchmarks verified.');
}
