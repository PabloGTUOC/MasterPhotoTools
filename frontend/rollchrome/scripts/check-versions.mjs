/**
 * RP-6 Virtual copies acceptance & verification in a real headless browser.
 *
 * Asserts all 14 requirements as numbered in the brief:
 * 1. A photograph with no versions shows Main alone and offers no Delete and no Export all.
 * 2. New version with a name makes it current; the save payload has the old one in versions and version_name set.
 * 3. Switching exchanges the recipes: the sliders show the other version's values, a render and a save with them are sent.
 * 4. Editing one version leaves the other's values intact when switching back.
 * 5. Switching empties the history list.
 * 6. A snapshot is still listed after switching, and after switching back.
 * 7. Rename of the current and of another version reach the save payload; an empty and a duplicate name, and Main while it is taken, are refused with a visible message.
 * 8. Deleting another version removes it from the save payload.
 * 9. Export sends the current recipe, with version_name.
 * 10. Export all with three versions sends three exports with no two overlapping; with one stubbed failure the other two are still sent, and the summary shows two written and one failure with its reason.
 * 11. Pasting settings from a photograph with versions brings none, and leaves the current name.
 * 12. Undo after switching does not change the list of versions or the name.
 * 13. Apply a preset, then move a slider: the notice is gone. Cmd-Z still takes back the slider, then the preset.
 * 14. Every new control is at least 40 px. Screenshot to layout-proof/.
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

console.log('Starting Vite harness server for check:versions...');
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
page.on('console', (msg) => {
  if (msg.text().includes('[DEBUG]')) console.log('BROWSER:', msg.text());
});

const failures = [];
let assertionsRun = 0;

try {
  await page.goto(base, { waitUntil: 'networkidle' });
  const pathInput = page.locator('input[placeholder="/path/to/image.jpg"]');
  await pathInput.waitFor({ state: 'visible' });

  const blurAll = async () => {
    await page.evaluate(() => {
      if (document.activeElement && document.activeElement instanceof HTMLElement) {
        document.activeElement.blur();
      }
      window.getSelection()?.removeAllRanges();
    });
  };

  const pressUndo = async () => {
    await blurAll();
    await page.keyboard.press('Meta+z');
  };

  const setSliderValue = async (testId, value) => {
    const input = page.locator(`input[data-testid="${testId}-number"]`);
    await input.fill(String(value));
    await input.dispatchEvent('change');
    await blurAll();
  };

  const openVersionsSection = async () => {
    const secHeader = page.locator('[data-testid="section-versions"] .collapsible-section__header');
    const panel = page.locator('[data-testid="versions-panel"]');
    if (!(await panel.isVisible())) {
      await secHeader.click();
      await page.waitForTimeout(150);
    }
  };

  const openSnapshotsSection = async () => {
    const snapSecHeader = page.locator('[data-testid="section-snapshots"] .collapsible-section__header');
    const snapPanel = page.locator('[data-testid="snapshots-panel"]');
    if (!(await snapPanel.isVisible())) {
      await snapSecHeader.click();
      await page.waitForTimeout(150);
    }
  };

  const openHistorySection = async () => {
    const histSecHeader = page.locator('[data-testid="section-history"] .collapsible-section__header');
    const histPanel = page.locator('[data-testid="history-panel"]');
    if (!(await histPanel.isVisible())) {
      await histSecHeader.click();
      await page.waitForTimeout(150);
    }
  };

  // -------------------------------------------------------------------------
  // 1. A photograph with no versions shows Main alone and offers no Delete and no Export all.
  // -------------------------------------------------------------------------
  console.log('Asserting 1: A photograph with no versions shows Main alone and offers no Delete and no Export all...');
  assertionsRun++;
  try {
    await pathInput.fill('/Volumes/Photos/photo1.jpg');
    await pathInput.press('Enter');
    await page.waitForTimeout(300);
    await openVersionsSection();

    const currentName = await page.locator('[data-testid="current-version-name"]').textContent();
    assert.equal(currentName?.trim(), 'Main', 'Current version name must be Main when unnamed');

    const emptyMsg = page.locator('[data-testid="versions-empty"]');
    assert.ok(await emptyMsg.isVisible(), 'Versions list should be empty');

    const deleteBtns = await page.locator('[data-testid^="version-delete-"]').count();
    assert.equal(deleteBtns, 0, 'No Delete button should be offered when only Main exists');

    const exportAllBtn = page.locator('[data-testid="export-all-btn"]');
    assert.ok(!(await exportAllBtn.isVisible()), 'Export all button should not be offered when only 1 version exists');
  } catch (err) {
    failures.push({ assertion: 1, error: err });
    console.error('Assertion 1 failed:', err);
  }

  // -------------------------------------------------------------------------
  // 2. New version with a name makes it current; the save payload has the old one in versions and version_name set.
  // -------------------------------------------------------------------------
  console.log('Asserting 2: New version with a name makes it current; the save payload has the old one in versions and version_name set...');
  assertionsRun++;
  try {
    // Modify exposure on Main first
    await setSliderValue('exposure', 0.5);
    await page.waitForTimeout(400);

    // Create a new version
    const nameInput = page.locator('input[data-testid="version-name-input"]');
    await nameInput.fill('B&W Contrast');
    const createBtn = page.locator('button[data-testid="version-create-btn"]');
    await createBtn.click();
    await page.waitForTimeout(400);

    const currentName = await page.locator('[data-testid="current-version-name"]').textContent();
    assert.equal(currentName?.trim(), 'B&W Contrast', 'New version name must become current');

    // Verify save payload
    const lastSaved = await page.evaluate(() => window.__STUB__.lastSavedRecipe);
    assert.ok(lastSaved, 'A save should have occurred');
    assert.equal(lastSaved.version_name, 'B&W Contrast', 'Saved recipe version_name must be B&W Contrast');
    assert.ok(Array.isArray(lastSaved.versions), 'Saved recipe must have versions array');
    assert.equal(lastSaved.versions.length, 1, 'Saved recipe must carry the old version in versions');
    assert.equal(lastSaved.versions[0].name, 'Main', 'Old version name must be Main');
    assert.equal(lastSaved.versions[0].recipe.exposure, 0.5, 'Old version recipe must retain exposure 0.5');
  } catch (err) {
    failures.push({ assertion: 2, error: err });
    console.error('Assertion 2 failed:', err);
  }

  // -------------------------------------------------------------------------
  // 3. Switching exchanges the recipes: the sliders show the other version's values, a render and a save with them are sent.
  // -------------------------------------------------------------------------
  console.log('Asserting 3: Switching exchanges the recipes: the sliders show the other version\'s values, a render and a save with them are sent...');
  assertionsRun++;
  try {
    // Edit the new version B&W Contrast
    await setSliderValue('contrast', 40.0);
    await page.waitForTimeout(400);

    const renderCountBefore = await page.evaluate(() => window.__STUB__.renders.length);
    const saveCountBefore = await page.evaluate(() => window.__STUB__.saveCalls.length);

    // Switch back to Main
    const switchMainBtn = page.locator('button[data-testid="version-switch-Main"]');
    await switchMainBtn.click();
    await page.waitForTimeout(400);

    const currentName = await page.locator('[data-testid="current-version-name"]').textContent();
    assert.equal(currentName?.trim(), 'Main', 'Current version must now be Main');

    // Main has exposure 0.5 and contrast 0
    const expVal = await page.locator('input[data-testid="exposure-number"]').inputValue();
    const contrastVal = await page.locator('input[data-testid="contrast-number"]').inputValue();
    assert.equal(Number(expVal), 0.5, 'Main exposure slider must show 0.5');
    assert.equal(Number(contrastVal), 0, 'Main contrast slider must show 0');

    // A render and save were sent
    const renderCountAfter = await page.evaluate(() => window.__STUB__.renders.length);
    const saveCountAfter = await page.evaluate(() => window.__STUB__.saveCalls.length);
    assert.ok(renderCountAfter > renderCountBefore, 'A render should have been sent on switch');
    assert.ok(saveCountAfter > saveCountBefore, 'A save should have been sent on switch');

    const lastSaved = await page.evaluate(() => window.__STUB__.lastSavedRecipe);
    assert.equal(lastSaved.version_name ?? null, null, 'Main version_name is null in save payload');
    assert.equal(lastSaved.exposure, 0.5, 'Main exposure 0.5 in save payload');
    assert.equal(lastSaved.versions.length, 1, 'versions array has 1 version');
    assert.equal(lastSaved.versions[0].name, 'B&W Contrast');
    assert.equal(lastSaved.versions[0].recipe.contrast, 40.0, 'B&W Contrast has contrast 40.0 in save payload');
  } catch (err) {
    failures.push({ assertion: 3, error: err });
    console.error('Assertion 3 failed:', err);
  }

  // -------------------------------------------------------------------------
  // 4. Editing one version leaves the other's values intact when switching back.
  // -------------------------------------------------------------------------
  console.log('Asserting 4: Editing one version leaves the other\'s values intact when switching back...');
  assertionsRun++;
  try {
    // Edit Main: temperature = 25
    await setSliderValue('temperature', 25.0);
    await page.waitForTimeout(400);

    // Switch to B&W Contrast
    const switchBwBtn = page.locator('button[data-testid="version-switch-B&W Contrast"]');
    await switchBwBtn.click();
    await page.waitForTimeout(400);

    // B&W Contrast should have contrast 40 and temperature 0
    const contrastBw = await page.locator('input[data-testid="contrast-number"]').inputValue();
    const tempBw = await page.locator('input[data-testid="temperature-number"]').inputValue();
    assert.equal(Number(contrastBw), 40.0, 'B&W Contrast contrast must remain 40');
    assert.equal(Number(tempBw), 0, 'B&W Contrast temperature must remain 0');

    // Switch back to Main
    const switchMainBtn = page.locator('button[data-testid="version-switch-Main"]');
    await switchMainBtn.click();
    await page.waitForTimeout(400);

    // Main should have temperature 25 and contrast 0
    const tempMain = await page.locator('input[data-testid="temperature-number"]').inputValue();
    const contrastMain = await page.locator('input[data-testid="contrast-number"]').inputValue();
    assert.equal(Number(tempMain), 25.0, 'Main temperature must remain 25');
    assert.equal(Number(contrastMain), 0, 'Main contrast must remain 0');
  } catch (err) {
    failures.push({ assertion: 4, error: err });
    console.error('Assertion 4 failed:', err);
  }

  // -------------------------------------------------------------------------
  // 5. Switching empties the history list.
  // -------------------------------------------------------------------------
  console.log('Asserting 5: Switching empties the history list...');
  assertionsRun++;
  try {
    // Make an edit on Main so history has items
    await setSliderValue('saturation', 15.0);
    await page.waitForTimeout(400);

    await openHistorySection();
    const histItemsBefore = await page.locator('.history-item').count();
    assert.ok(histItemsBefore > 0, 'History should have entries before switching');

    // Switch to B&W Contrast
    await openVersionsSection();
    const switchBwBtn = page.locator('button[data-testid="version-switch-B&W Contrast"]');
    await switchBwBtn.click();
    await page.waitForTimeout(400);

    // History should now be empty
    await openHistorySection();
    const histEmpty = page.locator('[data-testid="history-empty"]');
    assert.ok(await histEmpty.isVisible(), 'History list must be empty after switching versions');
    const histItemsAfter = await page.locator('.history-item').count();
    assert.equal(histItemsAfter, 0, 'Zero history items after switching versions');
  } catch (err) {
    failures.push({ assertion: 5, error: err });
    console.error('Assertion 5 failed:', err);
  }

  // -------------------------------------------------------------------------
  // 6. A snapshot is still listed after switching, and after switching back.
  // -------------------------------------------------------------------------
  console.log('Asserting 6: A snapshot is still listed after switching, and after switching back...');
  assertionsRun++;
  try {
    await openSnapshotsSection();
    const snapInput = page.locator('input[data-testid="snapshot-name-input"]');
    await snapInput.fill('GoldenHour');
    const snapSaveBtn = page.locator('button[data-testid="snapshot-save-btn"]');
    await snapSaveBtn.click();
    await page.waitForTimeout(200);

    const snapItem1 = page.locator('[data-testid="snapshot-item-GoldenHour"]');
    assert.ok(await snapItem1.isVisible(), 'Snapshot GoldenHour must be listed');

    // Switch to Main
    await openVersionsSection();
    const switchMainBtn = page.locator('button[data-testid="version-switch-Main"]');
    await switchMainBtn.click();
    await page.waitForTimeout(400);

    // Snapshot still listed
    await openSnapshotsSection();
    const snapItem2 = page.locator('[data-testid="snapshot-item-GoldenHour"]');
    assert.ok(await snapItem2.isVisible(), 'Snapshot GoldenHour must still be listed after switching to Main');

    // Switch back to B&W Contrast
    await openVersionsSection();
    const switchBwBtn = page.locator('button[data-testid="version-switch-B&W Contrast"]');
    await switchBwBtn.click();
    await page.waitForTimeout(400);

    // Snapshot still listed
    await openSnapshotsSection();
    const snapItem3 = page.locator('[data-testid="snapshot-item-GoldenHour"]');
    assert.ok(await snapItem3.isVisible(), 'Snapshot GoldenHour must still be listed after switching back');
  } catch (err) {
    failures.push({ assertion: 6, error: err });
    console.error('Assertion 6 failed:', err);
  }

  // -------------------------------------------------------------------------
  // 7. Rename of the current and of another version reach the save payload; an empty and a duplicate name, and Main while it is taken, are refused with a visible message.
  // -------------------------------------------------------------------------
  console.log('Asserting 7: Rename of the current and of another version reach the save payload; an empty and a duplicate name, and Main while it is taken, are refused with a visible message...');
  assertionsRun++;
  try {
    await openVersionsSection();

    // Currently current is B&W Contrast, other is Main
    // 1. Rename current version tests
    const renameCurrentBtn = page.locator('button[data-testid="rename-current-btn"]');
    await renameCurrentBtn.click();
    await page.waitForTimeout(100);

    const renameCurrentInput = page.locator('input[data-testid="rename-current-input"]');
    const renameCurrentSave = page.locator('button[data-testid="rename-current-save-btn"]');
    const versionMsg = page.locator('[data-testid="version-message"]');

    // Empty name
    await renameCurrentInput.fill('   ');
    await renameCurrentSave.click();
    await page.waitForTimeout(100);
    assert.ok(await versionMsg.isVisible(), 'Empty name must show message');
    assert.ok((await versionMsg.textContent())?.includes('empty'));

    // Main while Main is taken by other version
    await renameCurrentInput.fill('Main');
    await renameCurrentSave.click();
    await page.waitForTimeout(100);
    assert.ok(await versionMsg.isVisible(), 'Name Main while taken must show message');
    assert.ok((await versionMsg.textContent())?.includes('already exists'));

    // Valid rename of current version
    await renameCurrentInput.fill('Noir Monochrome');
    await renameCurrentSave.click();
    await page.waitForTimeout(400);

    const curName = await page.locator('[data-testid="current-version-name"]').textContent();
    assert.equal(curName?.trim(), 'Noir Monochrome');
    const savedAfterCurRename = await page.evaluate(() => window.__STUB__.lastSavedRecipe);
    assert.equal(savedAfterCurRename.version_name, 'Noir Monochrome', 'Save payload receives current version new name');

    // 2. Rename other version (Main)
    const renameOtherBtn = page.locator('button[data-testid="version-rename-Main"]');
    await renameOtherBtn.click();
    await page.waitForTimeout(100);

    const renameOtherInput = page.locator('input[data-testid="rename-input-Main"]');
    const renameOtherSave = page.locator('button[data-testid="rename-save-Main"]');

    // Duplicate name (Noir Monochrome)
    await renameOtherInput.fill('Noir Monochrome');
    await renameOtherSave.click();
    await page.waitForTimeout(100);
    assert.ok(await versionMsg.isVisible(), 'Duplicate name of current must show message');

    // Valid rename of other version
    await renameOtherInput.fill('Original Colour');
    await renameOtherSave.click();
    await page.waitForTimeout(400);

    const otherName = await page.locator('[data-testid="version-name-Original Colour"]').textContent();
    assert.equal(otherName?.trim(), 'Original Colour');
    const savedAfterOtherRename = await page.evaluate(() => window.__STUB__.lastSavedRecipe);
    assert.equal(savedAfterOtherRename.versions[0].name, 'Original Colour', 'Save payload receives other version new name');
  } catch (err) {
    failures.push({ assertion: 7, error: err });
    console.error('Assertion 7 failed:', err);
  }

  // -------------------------------------------------------------------------
  // 8. Deleting another version removes it from the save payload.
  // -------------------------------------------------------------------------
  console.log('Asserting 8: Deleting another version removes it from the save payload...');
  assertionsRun++;
  try {
    await openVersionsSection();
    const deleteBtn = page.locator('button[data-testid="version-delete-Original Colour"]');
    await deleteBtn.click();
    await page.waitForTimeout(400);

    const emptyMsg = page.locator('[data-testid="versions-empty"]');
    assert.ok(await emptyMsg.isVisible(), 'Versions list should now be empty');

    const lastSaved = await page.evaluate(() => window.__STUB__.lastSavedRecipe);
    assert.equal(lastSaved.versions.length, 0, 'Save payload has other version removed');
  } catch (err) {
    failures.push({ assertion: 8, error: err });
    console.error('Assertion 8 failed:', err);
  }

  // -------------------------------------------------------------------------
  // 9. Export sends the current recipe, with version_name.
  // -------------------------------------------------------------------------
  console.log('Asserting 9: Export sends the current recipe, with version_name...');
  assertionsRun++;
  try {
    // Current version is Noir Monochrome
    await page.evaluate(() => {
      window.__STUB__.exportCalls = [];
    });

    const exportBtn = page.locator('button[data-testid="export-btn"]');
    await exportBtn.click();
    await page.waitForTimeout(400);

    const exportCalls = await page.evaluate(() => window.__STUB__.exportCalls);
    assert.equal(exportCalls.length, 1, 'Exactly one export should have been sent');
    assert.equal(exportCalls[0].recipe.version_name, 'Noir Monochrome', 'Export must send recipe with version_name');
  } catch (err) {
    failures.push({ assertion: 9, error: err });
    console.error('Assertion 9 failed:', err);
  }

  // -------------------------------------------------------------------------
  // 10. Export all with three versions sends three exports with no two overlapping; with one stubbed failure the other two are still sent, and the summary shows two written and one failure with its reason.
  // -------------------------------------------------------------------------
  console.log('Asserting 10: Export all with three versions sends three exports with no two overlapping; with one stubbed failure the other two are still sent, and the summary shows two written and one failure with its reason...');
  assertionsRun++;
  try {
    await openVersionsSection();
    // Currently have 1 version (Noir Monochrome). Create two more: "Warm Sunset" and "Cool Moody".
    const nameInput = page.locator('input[data-testid="version-name-input"]');
    const createBtn = page.locator('button[data-testid="version-create-btn"]');

    await nameInput.fill('Warm Sunset');
    await createBtn.click();
    await page.waitForTimeout(300);

    await nameInput.fill('Cool Moody');
    await createBtn.click();
    await page.waitForTimeout(300);

    // Total versions now = 3 (current is Cool Moody, others are Noir Monochrome and Warm Sunset)
    await page.evaluate(() => {
      window.__STUB__.exportCalls = [];
      window.__STUB__.exportDelay = 60; // Introduce delay to verify sequential execution
      window.__STUB__.overlappingExportsDetected = false;
      window.__STUB__.activeExports = 0;
      window.__STUB__.exportFailureMap = {
        'Warm Sunset': 'Disk quota exceeded (simulated error)',
      };
    });

    const exportAllBtn = page.locator('button[data-testid="export-all-btn"]');
    assert.ok(await exportAllBtn.isVisible(), 'Export all button must be visible when versions count > 1');

    await exportAllBtn.click();

    // Wait for summary to appear
    const summaryEl = page.locator('[data-testid="export-all-summary"]');
    await summaryEl.waitFor({ state: 'visible', timeout: 10000 });
    const summaryText = await summaryEl.textContent();

    // Check overlapping and calls count
    const stubInfo = await page.evaluate(() => ({
      overlapping: window.__STUB__.overlappingExportsDetected,
      callsCount: window.__STUB__.exportCalls.length,
    }));

    assert.equal(stubInfo.overlapping, false, 'Exports must be sequential with no two overlapping');
    assert.equal(stubInfo.callsCount, 3, 'Three exports must be sent even with one failure');

    assert.ok(summaryText?.includes('2 written'), `Summary must show 2 written, got: ${summaryText}`);
    assert.ok(summaryText?.includes('1 failed'), `Summary must show 1 failed, got: ${summaryText}`);
    assert.ok(summaryText?.includes('Disk quota exceeded'), `Summary must include failure reason, got: ${summaryText}`);
  } catch (err) {
    failures.push({ assertion: 10, error: err });
    console.error('Assertion 10 failed:', err);
  }

  // -------------------------------------------------------------------------
  // 11. Pasting settings from a photograph with versions brings none, and leaves the current name.
  // -------------------------------------------------------------------------
  console.log('Asserting 11: Pasting settings from a photograph with versions brings none, and leaves the current name...');
  assertionsRun++;
  try {
    // Copy settings from current photograph (which has versions)
    await blurAll();
    await page.keyboard.press('Meta+c');
    await page.waitForTimeout(200);

    // Open a fresh photo
    await pathInput.fill('/Volumes/Photos/fresh_target.jpg');
    await pathInput.press('Enter');
    await page.waitForTimeout(300);
    await openVersionsSection();

    const curNameBefore = await page.locator('[data-testid="current-version-name"]').textContent();
    assert.equal(curNameBefore?.trim(), 'Main', 'Fresh image starts as Main');

    // Paste settings
    await blurAll();
    await page.keyboard.press('Meta+v');
    await page.waitForTimeout(400);

    const curNameAfter = await page.locator('[data-testid="current-version-name"]').textContent();
    assert.equal(curNameAfter?.trim(), 'Main', 'Pasting settings leaves current version name intact as Main');

    const emptyMsg = page.locator('[data-testid="versions-empty"]');
    assert.ok(await emptyMsg.isVisible(), 'Pasting settings brings no versions');

    const lastSaved = await page.evaluate(() => window.__STUB__.lastSavedRecipe);
    assert.equal(lastSaved.version_name ?? null, null, 'Saved recipe version_name remains null');
    assert.equal((lastSaved.versions ?? []).length, 0, 'Saved recipe versions array remains empty');
  } catch (err) {
    failures.push({ assertion: 11, error: err });
    console.error('Assertion 11 failed:', err);
  }

  // -------------------------------------------------------------------------
  // 12. Undo after switching does not change the list of versions or the name.
  // -------------------------------------------------------------------------
  console.log('Asserting 12: Undo after switching does not change the list of versions or the name...');
  assertionsRun++;
  try {
    await openVersionsSection();

    // Create a version Copy 1
    const nameInput = page.locator('input[data-testid="version-name-input"]');
    const createBtn = page.locator('button[data-testid="version-create-btn"]');
    await nameInput.fill('Copy 1');
    await createBtn.click();
    await page.waitForTimeout(300);

    // Current is Copy 1, other is Main. Switch to Main.
    const switchMainBtn = page.locator('button[data-testid="version-switch-Main"]');
    await switchMainBtn.click();
    await page.waitForTimeout(400);

    assert.equal((await page.locator('[data-testid="current-version-name"]').textContent())?.trim(), 'Main');

    // Press Undo
    await pressUndo();
    await page.waitForTimeout(200);

    // Current version must still be Main, and other version Copy 1 still present
    const curName = (await page.locator('[data-testid="current-version-name"]').textContent())?.trim();
    assert.equal(curName, 'Main', 'Undo after switching must not change current version name');

    const otherItem = page.locator('[data-testid="version-item-Copy 1"]');
    assert.ok(await otherItem.isVisible(), 'Undo after switching must not change versions list');
  } catch (err) {
    failures.push({ assertion: 12, error: err });
    console.error('Assertion 12 failed:', err);
  }

  // -------------------------------------------------------------------------
  // 13. Apply a preset, then move a slider: the notice is gone. Cmd-Z still takes back the slider, then the preset.
  // -------------------------------------------------------------------------
  console.log('Asserting 13: Apply a preset, then move a slider: the notice is gone. Cmd-Z still takes back the slider, then the preset...');
  assertionsRun++;
  try {
    // Open a photo to create a preset
    await pathInput.fill('/Volumes/Photos/preset_source.jpg');
    await pathInput.press('Enter');
    await page.waitForTimeout(300);

    // Save a preset WarmGlow with exposure 1.5
    await setSliderValue('exposure', 1.5);
    await page.waitForTimeout(400);
    await page.locator('button[data-testid="preset-save-btn"]').click();
    await page.waitForTimeout(100);
    await page.locator('input[data-testid="preset-name-input"]').fill('WarmGlow');
    await page.locator('button[data-testid="preset-submit-btn"]').click();
    await page.waitForTimeout(300);

    // Open a fresh photo for testing preset application
    await pathInput.fill('/Volumes/Photos/preset_target.jpg');
    await pathInput.press('Enter');
    await page.waitForTimeout(300);

    // Apply preset WarmGlow via preset select
    await page.locator('select[data-testid="preset-select"]').selectOption('WarmGlow');
    await page.waitForTimeout(300);

    // Verify notice is shown
    const notice = page.locator('[data-testid="edit-notice"]');
    assert.ok(await notice.isVisible(), 'Notice should be visible after applying preset');

    // Move a slider (contrast to 30)
    await setSliderValue('contrast', 30.0);
    await page.waitForTimeout(300);

    // Notice must be gone!
    assert.ok(!(await notice.isVisible()), 'Notice must be dismissed when a new step is recorded');

    // First Cmd-Z: takes back contrast slider
    await pressUndo();
    await page.waitForTimeout(300);
    const contrastAfter1 = await page.locator('input[data-testid="contrast-number"]').inputValue();
    const expAfter1 = await page.locator('input[data-testid="exposure-number"]').inputValue();
    assert.equal(Number(contrastAfter1), 0, 'First undo takes back slider change');
    assert.equal(Number(expAfter1), 1.5, 'Exposure remains at preset value after first undo');

    // Second Cmd-Z: takes back the preset
    await pressUndo();
    await page.waitForTimeout(300);
    const expAfter2 = await page.locator('input[data-testid="exposure-number"]').inputValue();
    assert.equal(Number(expAfter2), 0, 'Second undo takes back the preset');
  } catch (err) {
    failures.push({ assertion: 13, error: err });
    console.error('Assertion 13 failed:', err);
  }

  // -------------------------------------------------------------------------
  // 14. Every new control is at least 40 px. Screenshot to layout-proof/.
  // -------------------------------------------------------------------------
  console.log('Asserting 14: Every new control is at least 40 px. Screenshot to layout-proof/...');
  assertionsRun++;
  try {
    await openVersionsSection();

    const controlsToCheck = [
      { name: 'Versions section header', selector: '[data-testid="section-versions"] .collapsible-section__header' },
      { name: 'Current version rename button', selector: 'button[data-testid="rename-current-btn"]' },
      { name: 'Version name input', selector: 'input[data-testid="version-name-input"]' },
      { name: 'Version create button', selector: 'button[data-testid="version-create-btn"]' },
    ];

    // If another version exists, check its controls
    const switchBtns = page.locator('button[data-testid^="version-switch-"]');
    if ((await switchBtns.count()) > 0) {
      controlsToCheck.push(
        { name: 'Version switch button', selector: 'button[data-testid^="version-switch-"]' },
        { name: 'Version rename button', selector: 'button[data-testid^="version-rename-"]' },
        { name: 'Version delete button', selector: 'button[data-testid^="version-delete-"]' },
      );
    }

    const exportAllBtn = page.locator('button[data-testid="export-all-btn"]');
    if (await exportAllBtn.isVisible()) {
      controlsToCheck.push({ name: 'Export all button', selector: 'button[data-testid="export-all-btn"]' });
    }

    for (const ctrl of controlsToCheck) {
      const el = page.locator(ctrl.selector).first();
      const box = await el.boundingBox();
      assert.ok(box, `${ctrl.name} bounding box must exist`);
      assert.ok(
        box.height >= 39.5,
        `${ctrl.name} height must be at least 40px, got ${box.height}px`
      );
      assert.ok(
        box.width >= 39.5,
        `${ctrl.name} width must be at least 40px, got ${box.width}px`
      );
    }

    // Save proof screenshot
    const proofPath = join(OUT, 'versions.png');
    await page.screenshot({ path: proofPath, fullPage: true });
    console.log(`Saved screenshot to ${proofPath}`);
  } catch (err) {
    failures.push({ assertion: 14, error: err });
    console.error('Assertion 14 failed:', err);
  }

} finally {
  await browser.close();
  await server.close();
}

console.log(`\n========================================`);
console.log(`Assertions run: ${assertionsRun}/14`);
if (failures.length > 0) {
  console.error(`FAILURES: ${failures.length}`);
  for (const f of failures) {
    console.error(`  - Assertion ${f.assertion}: ${f.error.message}`);
  }
  process.exit(1);
} else {
  console.log(`All 14 assertions PASSED!`);
}
