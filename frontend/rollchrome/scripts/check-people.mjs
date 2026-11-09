/**
 * RP-11d Masks: People, Subject and Background, and Sky guard acceptance & verification.
 *
 * Implements all 7 assertions from docs/briefs/RP-11d.md:
 * 1. + People sends make_auto_mask with people; a mask People 1 of kind auto, target people,
 *    is selected, overlay shown, and in the save payload.
 * 2. Add component -> People makes a component with op: add and target people, the base shape unchanged.
 * 3. Model absent: the download is asked for with its name, and on completion People 1 is made.
 * 4. As 3, from Add component: a component, not a new mask.
 * 5. A refused Sky ("no sky was found in this photograph") creates no mask and shows those words.
 * 6. A refused Sky component leaves the mask's components unchanged and shows those words.
 * 7. Both new buttons are at least 40 px; screenshot to layout-proof/.
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

console.log('Starting Vite harness server for check:people...');
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

const executablePath = preinstalledChromium();
const browser = await chromium.launch({
  headless: true,
  executablePath,
  args: ['--no-sandbox', '--disable-setuid-sandbox', '--disable-dev-shm-usage'],
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

  const blurAll = async () => {
    await page.evaluate(() => {
      if (document.activeElement && document.activeElement instanceof HTMLElement) {
        document.activeElement.blur();
      }
      window.getSelection()?.removeAllRanges();
    });
  };

  const ensureMasksSectionOpen = async () => {
    const section = page.locator('[data-testid="section-masks"]');
    await section.waitFor({ state: 'visible' });
    const isOpen = await section.evaluate((el) => el.classList.contains('is-open'));
    if (!isOpen) {
      await page.locator('button[data-testid="section-masks-toggle"]').click();
      await page.waitForTimeout(100);
    }
  };

  // -------------------------------------------------------------------------
  // 1. + People sends make_auto_mask with people; a mask People 1 of kind auto, target people,
  //    is selected, overlay shown, and in the save payload.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 1: + People creates selected People 1 auto mask with overlay shown...');
  await page.evaluate(() => {
    window.__STUB__.modelsPresent = { subject: true, sky: true, people: true };
    window.__STUB__.autoMaskCalls = [];
    window.__STUB__.lastSavedRecipe = null;
    window.__STUB__.autoMaskFails = null;
  });
  await pathInput.fill('/Volumes/Photos/check_people_1.jpg');
  await pathInput.press('Enter');
  await page.waitForTimeout(250);
  await blurAll();
  await ensureMasksSectionOpen();

  const addPeopleBtn = page.locator('button[data-testid="add-people-mask"]');
  await addPeopleBtn.waitFor({ state: 'visible' });
  await addPeopleBtn.click();

  // Wait for mask creation
  await page.waitForFunction(() => {
    const m = window.__STUB__.lastSavedRecipe?.masks?.find((x) => x.name === 'People 1');
    return m && m.kind?.type === 'auto' && m.kind?.target === 'people';
  }, { timeout: 8000 });

  const calls1 = await page.evaluate(() => window.__STUB__.autoMaskCalls);
  if (!calls1.includes('people')) {
    failures.push(`Assertion 1: Expected autoMaskCalls to include 'people', got ${JSON.stringify(calls1)}`);
  }

  const saved1 = await page.evaluate(() => window.__STUB__.lastSavedRecipe?.masks);
  const peopleMask1 = saved1?.find((m) => m.name === 'People 1');
  if (!peopleMask1) {
    failures.push('Assertion 1: People 1 was not found in save payload');
  } else {
    if (peopleMask1.kind?.type !== 'auto' || peopleMask1.kind?.target !== 'people') {
      failures.push(`Assertion 1: Expected auto target 'people', got ${JSON.stringify(peopleMask1.kind)}`);
    }
  }

  // Selected: mask editing header shows 'People 1'
  const maskEditing = page.locator('[data-testid="mask-editing"]');
  await maskEditing.waitFor({ state: 'visible' });
  const title = await page.locator('[data-testid="mask-editing-name"]').textContent();
  if (!title?.includes('People 1')) {
    failures.push(`Assertion 1: Expected mask editing name to include 'People 1', got '${title}'`);
  }

  // Overlay shown
  const overlayBtn = page.locator('button[data-testid="mask-show-overlay"]');
  await overlayBtn.waitFor({ state: 'visible' });
  const isOverlayActive = await overlayBtn.evaluate((el) => el.classList.contains('active'));
  if (!isOverlayActive) {
    failures.push('Assertion 1: Mask overlay button does not have active class');
  }

  // -------------------------------------------------------------------------
  // 2. Add component -> People makes a component with op: add and target people,
  //    base shape unchanged.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 2: Add component -> People adds auto component with op: add...');
  const addCompPeopleBtn = page.locator('button[data-testid="add-component-people"]');
  await addCompPeopleBtn.waitFor({ state: 'visible' });
  await addCompPeopleBtn.click();

  await page.waitForFunction(() => {
    const m = window.__STUB__.lastSavedRecipe?.masks?.find((x) => x.name === 'People 1');
    return m?.components?.some((c) => c.op === 'add' && c.kind?.type === 'auto' && c.kind?.target === 'people');
  }, { timeout: 8000 });

  const saved2 = await page.evaluate(() => window.__STUB__.lastSavedRecipe?.masks?.find((m) => m.name === 'People 1'));
  if (!saved2) {
    failures.push('Assertion 2: People 1 mask missing after adding component');
  } else {
    if (saved2.kind?.type !== 'auto' || saved2.kind?.target !== 'people') {
      failures.push('Assertion 2: Base mask shape changed when component was added');
    }
    const comp = saved2.components?.find((c) => c.kind?.type === 'auto' && c.kind?.target === 'people');
    if (!comp || comp.op !== 'add') {
      failures.push(`Assertion 2: Expected component with op: add, got: ${JSON.stringify(comp)}`);
    }
  }

  // -------------------------------------------------------------------------
  // 3. Model absent: the download is asked for with its name, and on completion People 1 is made.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 3: Model absent prompts for download with name, creates People 1 on completion...');
  await page.evaluate(() => {
    window.__STUB__.modelsPresent = { subject: true, sky: true, people: false };
    window.__STUB__.autoMaskCalls = [];
    window.__STUB__.lastSavedRecipe = null;
  });
  await pathInput.fill('/Volumes/Photos/check_people_dl.jpg');
  await pathInput.press('Enter');
  await page.waitForTimeout(250);
  await blurAll();
  await ensureMasksSectionOpen();

  const addPeopleDlBtn = page.locator('button[data-testid="add-people-mask"]');
  await addPeopleDlBtn.waitFor({ state: 'visible' });
  await addPeopleDlBtn.click();
  await page.waitForTimeout(200);

  const autoDlCard = page.locator('[data-testid="auto-download"]');
  await autoDlCard.waitFor({ state: 'visible' });
  const dlText = await autoDlCard.textContent();
  if (!dlText?.includes('BiRefNet portrait (MIT), 973 MB')) {
    failures.push(`Assertion 3: Expected download banner to name 'BiRefNet portrait (MIT), 973 MB', got: '${dlText}'`);
  }

  // Start download
  const dlStartBtn = page.locator('button[data-testid="auto-download-start"]');
  await dlStartBtn.click();

  // On completion, People 1 mask is made
  await page.waitForFunction(() => {
    const m = window.__STUB__.lastSavedRecipe?.masks?.find((x) => x.name === 'People 1');
    return m && m.kind?.type === 'auto' && m.kind?.target === 'people';
  }, { timeout: 8000 });

  const saved3 = await page.evaluate(() => window.__STUB__.lastSavedRecipe?.masks);
  const peopleMask3 = saved3?.find((m) => m.name === 'People 1');
  if (!peopleMask3) {
    failures.push('Assertion 3: People 1 was not created after model download completed');
  }

  // -------------------------------------------------------------------------
  // 4. As 3, from Add component: a component, not a new mask.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 4: Component download creates a component on completion, not a new mask...');
  // People 1 is currently selected. Set people model absent again.
  await page.evaluate(() => {
    window.__STUB__.modelsPresent.people = false;
  });

  const maskCountBefore4 = await page.evaluate(() => window.__STUB__.lastSavedRecipe?.masks?.length ?? 1);

  const addCompPeopleDlBtn = page.locator('button[data-testid="add-component-people"]');
  await addCompPeopleDlBtn.waitFor({ state: 'visible' });
  await addCompPeopleDlBtn.click();
  await page.waitForTimeout(200);

  // Auto download card appears
  const autoDlCard4 = page.locator('[data-testid="auto-download"]');
  await autoDlCard4.waitFor({ state: 'visible' });
  const dlText4 = await autoDlCard4.textContent();
  if (!dlText4?.includes('BiRefNet portrait (MIT), 973 MB')) {
    failures.push(`Assertion 4: Expected download banner to name 'BiRefNet portrait (MIT), 973 MB', got: '${dlText4}'`);
  }

  const dlStartBtn4 = page.locator('button[data-testid="auto-download-start"]');
  await dlStartBtn4.click();

  // Wait for completion: component added
  await page.waitForFunction(() => {
    const m = window.__STUB__.lastSavedRecipe?.masks?.find((x) => x.name === 'People 1');
    return m?.components?.some((c) => c.kind?.type === 'auto' && c.kind?.target === 'people');
  }, { timeout: 8000 });

  const recipeAfterCompDl = await page.evaluate(() => window.__STUB__.lastSavedRecipe);
  const maskCountAfter4 = recipeAfterCompDl?.masks?.length ?? 0;
  if (maskCountAfter4 !== maskCountBefore4) {
    failures.push(`Assertion 4: Model download for component added a separate mask instead of a component (mask count went from ${maskCountBefore4} to ${maskCountAfter4})`);
  }

  // -------------------------------------------------------------------------
  // 5. A refused Sky ("no sky was found in this photograph") creates no mask and shows those words.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 5: Refused Sky creates no mask and displays error message...');
  await page.evaluate(() => {
    window.__STUB__.modelsPresent = { subject: true, sky: true, people: true };
    window.__STUB__.autoMaskFails = 'no sky was found in this photograph';
    window.__STUB__.lastSavedRecipe = null;
  });
  await pathInput.fill('/Volumes/Photos/check_nosky.jpg');
  await pathInput.press('Enter');
  await page.waitForTimeout(250);
  await blurAll();
  await ensureMasksSectionOpen();

  const addSkyBtn = page.locator('button[data-testid="add-sky-mask"]');
  await addSkyBtn.waitFor({ state: 'visible' });
  await addSkyBtn.click();

  const errorEl5 = page.locator('[data-testid="edit-error"]');
  await errorEl5.waitFor({ state: 'visible', timeout: 5000 });
  const errText5 = await errorEl5.textContent();
  if (!errText5?.includes('no sky was found in this photograph')) {
    failures.push(`Assertion 5: Expected error text to contain 'no sky was found in this photograph', got: '${errText5}'`);
  }

  const masksAfterRefusal = await page.evaluate(() => window.__STUB__.lastSavedRecipe?.masks ?? []);
  if (masksAfterRefusal.length !== 0) {
    failures.push(`Assertion 5: Expected 0 masks created after refused Sky, got ${masksAfterRefusal.length}`);
  }

  // Clear refusal
  await page.evaluate(() => {
    window.__STUB__.autoMaskFails = null;
  });

  // -------------------------------------------------------------------------
  // 6. A refused Sky component leaves the mask's components unchanged and shows those words.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 6: Refused Sky component leaves mask components unchanged and displays words...');
  // Create a mask first (e.g. + People)
  const addPeopleBtn6 = page.locator('button[data-testid="add-people-mask"]');
  await addPeopleBtn6.waitFor({ state: 'visible' });
  await addPeopleBtn6.click();

  await page.waitForFunction(() => {
    return !!window.__STUB__.lastSavedRecipe?.masks?.find((m) => m.name === 'People 1');
  }, { timeout: 8000 });

  const compCountBefore6 = await page.evaluate(() => {
    const m = window.__STUB__.lastSavedRecipe?.masks?.find((x) => x.name === 'People 1');
    return m?.components?.length ?? 0;
  });

  // Now set Sky to fail
  await page.evaluate(() => {
    window.__STUB__.autoMaskFails = 'no sky was found in this photograph';
  });

  const addCompSkyBtn = page.locator('button[data-testid="add-component-sky"]');
  await addCompSkyBtn.waitFor({ state: 'visible' });
  await addCompSkyBtn.click();

  const errorEl6 = page.locator('[data-testid="edit-error"]');
  await errorEl6.waitFor({ state: 'visible', timeout: 5000 });
  const errText6 = await errorEl6.textContent();
  if (!errText6?.includes('no sky was found in this photograph')) {
    failures.push(`Assertion 6: Expected error text to contain 'no sky was found in this photograph', got: '${errText6}'`);
  }

  const compCountAfter6 = await page.evaluate(() => {
    const m = window.__STUB__.lastSavedRecipe?.masks?.find((x) => x.name === 'People 1');
    return m?.components?.length ?? 0;
  });
  if (compCountAfter6 !== compCountBefore6) {
    failures.push(`Assertion 6: Components count changed from ${compCountBefore6} to ${compCountAfter6} on refused component`);
  }

  // -------------------------------------------------------------------------
  // 7. Both new buttons are at least 40 px; screenshot to layout-proof/.
  // -------------------------------------------------------------------------
  assertionsRun += 1;
  console.log('Asserting 7: Both new buttons are >= 40px touch targets...');
  const buttonsToCheck = [
    'add-people-mask',
    'add-component-people',
  ];

  for (const tid of buttonsToCheck) {
    const loc = page.locator(`button[data-testid="${tid}"]`).first();
    const visible = await loc.isVisible();
    if (!visible) {
      failures.push(`Assertion 7: Required control ${tid} is missing or not visible`);
      continue;
    }
    const box = await loc.boundingBox();
    if (!box || box.width < 39.5 || box.height < 39.5) {
      failures.push(`Assertion 7: Control ${tid} touch target is under 40px (${box?.width?.toFixed(1)}x${box?.height?.toFixed(1)})`);
    }
  }

  const proofPath = join(OUT, 'check-people.png');
  await page.screenshot({ path: proofPath });
  console.log(`Saved screenshot proof to ${proofPath}`);

} catch (err) {
  failures.push(`Unexpected error during check:people execution: ${err.message}\n${err.stack}`);
} finally {
  await browser.close();
  await server.close();
}

console.log('\n--- check:people Summary ---');
console.log(`Assertions executed: ${assertionsRun}/7`);
if (failures.length === 0) {
  console.log('ALL 7 ASSERTIONS PASSED');
  process.exit(0);
} else {
  console.error(`FAILURES (${failures.length}):`);
  for (const f of failures) {
    console.error(`  - ${f}`);
  }
  process.exit(1);
}
