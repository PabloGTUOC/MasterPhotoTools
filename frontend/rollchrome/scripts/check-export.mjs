/**
 * RP-9 Export options acceptance & verification in a real headless browser.
 *
 * Asserts all 11 requirements as numbered in the brief:
 * 1. At the defaults, the call sends the default options, and the section shows Full, As source.
 * 2. Each format sends its own form: JPEG with quality, TIFF and PNG with bits, WebP, AVIF with quality.
 * 3. The quality control appears only for JPEG and AVIF, the bits control only for TIFF and PNG.
 * 4. WebP says it is lossless.
 * 5. Long edge and Megapixels send their number; an empty or zero number disables Export with a reason.
 * 6. Output sharpening and metadata send their choices.
 * 7. A template is sent, and the example shows the substituted name, using the current version.
 * 8. A refused template from the stub is shown.
 * 9. Leaving the tab and returning keeps the settings.
 * 10. Export all sends the same options for each version.
 * 11. Every new control is at least 40 px. Screenshot to layout-proof/.
 */

import { existsSync, mkdirSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath, URL } from 'node:url';
import assert from 'node:assert/strict';
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

console.log('Starting Vite harness server for check:export...');
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

  // Open an image in harness
  await pathInput.fill('/Volumes/Photos/IMG_0001.jpg');
  await pathInput.press('Enter');
  await page.waitForTimeout(300);

  const formatSelect = page.locator('[data-testid="export-format-select"]');
  const sizeSelect = page.locator('[data-testid="export-size-select"]');
  const exportBtn = page.locator('[data-testid="export-btn"]');

  // Helper to clear recorded export calls in stub
  const resetExportCalls = async () => {
    await page.evaluate(() => {
      window.__STUB__.exportCalls = [];
    });
  };

  const getLastExportCall = async () => {
    return await page.evaluate(() => {
      const calls = window.__STUB__.exportCalls;
      return calls.length > 0 ? calls[calls.length - 1] : null;
    });
  };

  // -------------------------------------------------------------------------
  // 1. At the defaults, the call sends the default options, and the section shows Full, As source.
  // -------------------------------------------------------------------------
  console.log('Asserting 1: At the defaults, the call sends the default options, and the section shows Full, As source...');
  assertionsRun++;
  try {
    const formatVal = await formatSelect.inputValue();
    const sizeVal = await sizeSelect.inputValue();
    assert.equal(formatVal, 'AsSource', 'Format select must default to AsSource');
    assert.equal(sizeVal, 'Full', 'Size select must default to Full');

    await resetExportCalls();
    await exportBtn.click();
    await page.waitForTimeout(100);

    const call = await getLastExportCall();
    assert.ok(call, 'Export call must be received');
    assert.deepEqual(
      call.options,
      {
        format: 'AsSource',
        resize: 'None',
        output_sharpening: 'Off',
        name_template: null,
        metadata: 'All',
      },
      'Default export options must match backend defaults'
    );
  } catch (err) {
    failures.push({ assertion: 1, error: err });
    console.error('Assertion 1 failed:', err);
  }

  // -------------------------------------------------------------------------
  // 2. Each format sends its own form: JPEG with quality, TIFF and PNG with bits, WebP, AVIF with quality.
  // -------------------------------------------------------------------------
  console.log('Asserting 2: Each format sends its own form: JPEG with quality, TIFF and PNG with bits, WebP, AVIF with quality...');
  assertionsRun++;
  try {
    // JPEG with quality 85
    await formatSelect.selectOption('Jpeg');
    const qualityInput = page.locator('[data-testid="export-quality-input"]');
    await qualityInput.fill('85');
    await resetExportCalls();
    await exportBtn.click();
    await page.waitForTimeout(50);
    let call = await getLastExportCall();
    assert.deepEqual(call.options.format, { Jpeg: { quality: 85 } }, 'JPEG sends quality');

    // TIFF with bits 8
    await formatSelect.selectOption('Tiff');
    const bitsSelect = page.locator('[data-testid="export-bits-select"]');
    await bitsSelect.selectOption('8');
    await resetExportCalls();
    await exportBtn.click();
    await page.waitForTimeout(50);
    call = await getLastExportCall();
    assert.deepEqual(call.options.format, { Tiff: { bits: 8 } }, 'TIFF sends bits');

    // PNG with bits 16
    await formatSelect.selectOption('Png');
    await bitsSelect.selectOption('16');
    await resetExportCalls();
    await exportBtn.click();
    await page.waitForTimeout(50);
    call = await getLastExportCall();
    assert.deepEqual(call.options.format, { Png: { bits: 16 } }, 'PNG sends bits');

    // WebP
    await formatSelect.selectOption('Webp');
    await resetExportCalls();
    await exportBtn.click();
    await page.waitForTimeout(50);
    call = await getLastExportCall();
    assert.equal(call.options.format, 'Webp', 'WebP sends Webp');

    // AVIF with quality 65
    await formatSelect.selectOption('Avif');
    await qualityInput.fill('65');
    await resetExportCalls();
    await exportBtn.click();
    await page.waitForTimeout(50);
    call = await getLastExportCall();
    assert.deepEqual(call.options.format, { Avif: { quality: 65 } }, 'AVIF sends quality');
  } catch (err) {
    failures.push({ assertion: 2, error: err });
    console.error('Assertion 2 failed:', err);
  }

  // -------------------------------------------------------------------------
  // 3. The quality control appears only for JPEG and AVIF, the bits control only for TIFF and PNG.
  // -------------------------------------------------------------------------
  console.log('Asserting 3: The quality control appears only for JPEG and AVIF, the bits control only for TIFF and PNG...');
  assertionsRun++;
  try {
    const qualityInput = page.locator('[data-testid="export-quality-input"]');
    const bitsSelect = page.locator('[data-testid="export-bits-select"]');

    // AsSource
    await formatSelect.selectOption('AsSource');
    assert.ok(!(await qualityInput.isVisible()), 'Quality hidden on AsSource');
    assert.ok(!(await bitsSelect.isVisible()), 'Bits hidden on AsSource');

    // Jpeg
    await formatSelect.selectOption('Jpeg');
    assert.ok(await qualityInput.isVisible(), 'Quality visible on Jpeg');
    assert.ok(!(await bitsSelect.isVisible()), 'Bits hidden on Jpeg');

    // Tiff
    await formatSelect.selectOption('Tiff');
    assert.ok(!(await qualityInput.isVisible()), 'Quality hidden on Tiff');
    assert.ok(await bitsSelect.isVisible(), 'Bits visible on Tiff');

    // Png
    await formatSelect.selectOption('Png');
    assert.ok(!(await qualityInput.isVisible()), 'Quality hidden on Png');
    assert.ok(await bitsSelect.isVisible(), 'Bits visible on Png');

    // Webp
    await formatSelect.selectOption('Webp');
    assert.ok(!(await qualityInput.isVisible()), 'Quality hidden on Webp');
    assert.ok(!(await bitsSelect.isVisible()), 'Bits hidden on Webp');

    // Avif
    await formatSelect.selectOption('Avif');
    assert.ok(await qualityInput.isVisible(), 'Quality visible on Avif');
    assert.ok(!(await bitsSelect.isVisible()), 'Bits hidden on Avif');
  } catch (err) {
    failures.push({ assertion: 3, error: err });
    console.error('Assertion 3 failed:', err);
  }

  // -------------------------------------------------------------------------
  // 4. WebP says it is lossless.
  // -------------------------------------------------------------------------
  console.log('Asserting 4: WebP says it is lossless...');
  assertionsRun++;
  try {
    await formatSelect.selectOption('Webp');
    const notice = page.locator('[data-testid="export-webp-notice"]');
    assert.ok(await notice.isVisible(), 'WebP notice must be visible');
    const text = await notice.textContent();
    assert.ok(text?.toLowerCase().includes('lossless'), 'WebP notice must state lossless');
  } catch (err) {
    failures.push({ assertion: 4, error: err });
    console.error('Assertion 4 failed:', err);
  }

  // -------------------------------------------------------------------------
  // 5. Long edge and Megapixels send their number; an empty or zero number disables Export with a reason.
  // -------------------------------------------------------------------------
  console.log('Asserting 5: Long edge and Megapixels send their number; an empty or zero number disables Export with a reason...');
  assertionsRun++;
  try {
    const sizeInput = page.locator('[data-testid="export-size-input"]');
    const sizeError = page.locator('[data-testid="export-size-error"]');

    // Long edge with 1600
    await sizeSelect.selectOption('LongEdge');
    assert.ok(await sizeInput.isVisible(), 'Size input visible on LongEdge');
    await sizeInput.fill('1600');
    await resetExportCalls();
    await exportBtn.click();
    await page.waitForTimeout(50);
    let call = await getLastExportCall();
    assert.deepEqual(call.options.resize, { LongEdge: 1600 }, 'Long edge sends its number');

    // Megapixels with 12
    await sizeSelect.selectOption('Megapixels');
    await sizeInput.fill('12');
    await resetExportCalls();
    await exportBtn.click();
    await page.waitForTimeout(50);
    call = await getLastExportCall();
    assert.deepEqual(call.options.resize, { Megapixels: 12 }, 'Megapixels sends its number');

    // Empty number disables export
    await sizeInput.fill('');
    assert.ok(await exportBtn.isDisabled(), 'Export button must be disabled when size input is empty');
    assert.ok(await sizeError.isVisible(), 'Size error reason must be visible when empty');

    // Zero number disables export
    await sizeInput.fill('0');
    assert.ok(await exportBtn.isDisabled(), 'Export button must be disabled when size input is zero');
    assert.ok(await sizeError.isVisible(), 'Size error reason must be visible when zero');

    // Restore valid size
    await sizeInput.fill('2048');
    assert.ok(!(await exportBtn.isDisabled()), 'Export button must be re-enabled with positive size');
  } catch (err) {
    failures.push({ assertion: 5, error: err });
    console.error('Assertion 5 failed:', err);
  }

  // -------------------------------------------------------------------------
  // 6. Output sharpening and metadata send their choices.
  // -------------------------------------------------------------------------
  console.log('Asserting 6: Output sharpening and metadata send their choices...');
  assertionsRun++;
  try {
    const sharpSelect = page.locator('[data-testid="export-sharpening-select"]');
    const metaSelect = page.locator('[data-testid="export-metadata-select"]');

    await sharpSelect.selectOption('Screen');
    await metaSelect.selectOption('NoLocation');
    await resetExportCalls();
    await exportBtn.click();
    await page.waitForTimeout(50);
    let call = await getLastExportCall();
    assert.equal(call.options.output_sharpening, 'Screen');
    assert.equal(call.options.metadata, 'NoLocation');

    await sharpSelect.selectOption('Print');
    await metaSelect.selectOption('None');
    await resetExportCalls();
    await exportBtn.click();
    await page.waitForTimeout(50);
    call = await getLastExportCall();
    assert.equal(call.options.output_sharpening, 'Print');
    assert.equal(call.options.metadata, 'None');
  } catch (err) {
    failures.push({ assertion: 6, error: err });
    console.error('Assertion 6 failed:', err);
  }

  // -------------------------------------------------------------------------
  // 7. A template is sent, and the example shows the substituted name, using the current version.
  // -------------------------------------------------------------------------
  console.log('Asserting 7: A template is sent, and the example shows the substituted name, using the current version...');
  assertionsRun++;
  try {
    // Open versions section and create version "BW-Print"
    const secHeader = page.locator('[data-testid="section-versions"] .collapsible-section__header');
    const panel = page.locator('[data-testid="versions-panel"]');
    if (!(await panel.isVisible())) {
      await secHeader.click();
      await page.waitForTimeout(150);
    }
    const versionNameInput = page.locator('input[data-testid="version-name-input"]');
    await versionNameInput.fill('BW-Print');
    await page.locator('button[data-testid="version-create-btn"]').click();
    await page.waitForTimeout(200);

    const templateInput = page.locator('[data-testid="export-template-input"]');
    const nameExample = page.locator('[data-testid="export-name-example"]');
    await templateInput.fill('{date}_{name}_{version}_{edit}');
    await page.waitForTimeout(100);

    const exampleText = await nameExample.textContent();
    assert.ok(exampleText?.includes('IMG_0001'), 'Example must include stem');
    assert.ok(exampleText?.includes('BW-Print'), 'Example must include version name');
    assert.ok(exampleText?.includes('_edit_BW-Print'), 'Example must include edit suffix');

    await resetExportCalls();
    await exportBtn.click();
    await page.waitForTimeout(50);
    const call = await getLastExportCall();
    assert.equal(call.options.name_template, '{date}_{name}_{version}_{edit}');
  } catch (err) {
    failures.push({ assertion: 7, error: err });
    console.error('Assertion 7 failed:', err);
  }

  // -------------------------------------------------------------------------
  // 8. A refused template from the stub is shown.
  // -------------------------------------------------------------------------
  console.log('Asserting 8: A refused template from the stub is shown...');
  assertionsRun++;
  try {
    const templateInput = page.locator('[data-testid="export-template-input"]');
    await templateInput.fill('...');
    await exportBtn.click();
    await page.waitForTimeout(100);

    const errorDiv = page.locator('[data-testid="export-error"]');
    assert.ok(await errorDiv.isVisible(), 'Export error must be visible on refused template');
    const errText = await errorDiv.textContent();
    assert.ok(errText?.toLowerCase().includes('refused'), 'Error must display refused message');
  } catch (err) {
    failures.push({ assertion: 8, error: err });
    console.error('Assertion 8 failed:', err);
  }

  // -------------------------------------------------------------------------
  // 9. Leaving the tab and returning keeps the settings.
  // -------------------------------------------------------------------------
  console.log('Asserting 9: Leaving the tab and returning keeps the settings...');
  assertionsRun++;
  try {
    // Set distinctive options
    await formatSelect.selectOption('Avif');
    await sizeSelect.selectOption('Megapixels');
    await page.locator('[data-testid="export-size-input"]').fill('16');
    await page.locator('[data-testid="export-sharpening-select"]').selectOption('Screen');
    await page.locator('[data-testid="export-metadata-select"]').selectOption('NoLocation');
    await page.locator('[data-testid="export-template-input"]').fill('{name}_persistent');

    // Unmount Edit view (simulate navigating away to another tab)
    await page.evaluate(() => {
      window.__showEdit.value = false;
    });
    await page.waitForTimeout(100);
    assert.ok(await page.locator('#other-tab').isVisible(), 'Other tab should be mounted');
    assert.ok(!(await page.locator('[data-testid="export-format-select"]').isVisible()), 'Edit panel unmounted');

    // Mount Edit view again (return to tab)
    await page.evaluate(() => {
      window.__showEdit.value = true;
    });
    await page.waitForTimeout(100);

    // Re-open image after remounting
    await pathInput.fill('/Volumes/Photos/IMG_0001.jpg');
    await pathInput.press('Enter');
    await page.waitForTimeout(300);

    // Verify settings retained
    assert.equal(await page.locator('[data-testid="export-format-select"]').inputValue(), 'Avif');
    assert.equal(await page.locator('[data-testid="export-size-select"]').inputValue(), 'Megapixels');
    assert.equal(await page.locator('[data-testid="export-size-input"]').inputValue(), '16');
    assert.equal(await page.locator('[data-testid="export-sharpening-select"]').inputValue(), 'Screen');
    assert.equal(await page.locator('[data-testid="export-metadata-select"]').inputValue(), 'NoLocation');
    assert.equal(await page.locator('[data-testid="export-template-input"]').inputValue(), '{name}_persistent');
  } catch (err) {
    failures.push({ assertion: 9, error: err });
    console.error('Assertion 9 failed:', err);
  }

  // -------------------------------------------------------------------------
  // 10. Export all sends the same options for each version.
  // -------------------------------------------------------------------------
  console.log('Asserting 10: Export all sends the same options for each version...');
  assertionsRun++;
  try {
    // Ensure multiple versions exist
    const secHeader = page.locator('[data-testid="section-versions"] .collapsible-section__header');
    const panel = page.locator('[data-testid="versions-panel"]');
    if (!(await panel.isVisible())) {
      await secHeader.click();
      await page.waitForTimeout(150);
    }
    const versionNameInput = page.locator('input[data-testid="version-name-input"]');
    await versionNameInput.fill('V2');
    await page.locator('button[data-testid="version-create-btn"]').click();
    await page.waitForTimeout(200);

    await resetExportCalls();
    const exportAllBtn = page.locator('[data-testid="export-all-btn"]');
    assert.ok(await exportAllBtn.isVisible(), 'Export all button must be visible when multiple versions exist');
    await exportAllBtn.click();
    await page.waitForTimeout(200);

    const calls = await page.evaluate(() => window.__STUB__.exportCalls);
    assert.ok(calls.length >= 2, 'Export all must trigger exports for all versions');
    const firstOpts = calls[0].options;
    for (let i = 1; i < calls.length; i++) {
      assert.deepEqual(calls[i].options, firstOpts, `Export call ${i} options must match first call`);
    }
  } catch (err) {
    failures.push({ assertion: 10, error: err });
    console.error('Assertion 10 failed:', err);
  }

  // -------------------------------------------------------------------------
  // 11. Every new control is at least 40 px. Screenshot to layout-proof/.
  // -------------------------------------------------------------------------
  console.log('Asserting 11: Every new control is at least 40 px. Screenshot to layout-proof/...');
  assertionsRun++;
  try {
    const controls = [
      '[data-testid="export-format-select"]',
      '[data-testid="export-quality-input"]',
      '[data-testid="export-size-select"]',
      '[data-testid="export-size-input"]',
      '[data-testid="export-sharpening-select"]',
      '[data-testid="export-template-input"]',
      '[data-testid="export-metadata-select"]',
      '[data-testid="export-btn"]',
      '[data-testid="export-all-btn"]',
    ];

    for (const selector of controls) {
      const loc = page.locator(selector);
      if (await loc.isVisible()) {
        const box = await loc.boundingBox();
        assert.ok(box, `Bounding box must exist for ${selector}`);
        assert.ok(box.height >= 40, `Height for ${selector} must be >= 40px (got ${box.height})`);
        assert.ok(box.width >= 40, `Width for ${selector} must be >= 40px (got ${box.width})`);
      }
    }

    const proofPath = join(OUT, 'export.png');
    await page.screenshot({ path: proofPath, fullPage: true });
    assert.ok(existsSync(proofPath), 'Screenshot export.png must exist');
    console.log(`Saved layout proof to ${proofPath}`);
  } catch (err) {
    failures.push({ assertion: 11, error: err });
    console.error('Assertion 11 failed:', err);
  }

} finally {
  await browser.close();
  await server.close();
}

console.log(`\n========================================`);
console.log(`Ran ${assertionsRun} browser assertions. Failures: ${failures.length}`);
if (failures.length > 0) {
  for (const f of failures) {
    console.error(`FAILED Assertion ${f.assertion}:`, f.error);
  }
  process.exit(1);
} else {
  console.log('All 11 assertions PASSED.');
}
