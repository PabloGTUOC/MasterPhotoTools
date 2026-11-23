/**
 * UI-1 acceptance script: verify Rollchrome application shell and rail.
 *
 * Checks (at 1024×700, 1280×800 and 1920×1080):
 * 1. Develop: load folder, select photo, scroll content to bottom.
 *    Assert: no page scroll (scrollHeight <= innerHeight + 1), no horizontal scroll,
 *    sidebar top === 0 and height === innerHeight (±1), all 3 nav links fully inside viewport.
 * 2. Edit: load photo, expand all collapsible sections in adjustment panel,
 *    scroll content and panel to bottom. Assert same 4 invariants.
 * 3. Batch: scroll content to bottom. Assert same 4 invariants.
 * 4. Rail: sidebar is 56 ± 1 px wide, each nav link is at least 56 px tall,
 *    aria-labels match Develop, Edit, Batch, and clicking each navigates to #/, #/edit, #/batch.
 *
 * Screenshots written to layout-proof/shell-<screen>-<width>.png.
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

console.log('Starting Vite server for application shell harness...');
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
  console.error('\ncheck:shell FAILED: watchdog timeout after 8 minutes');
  process.exit(1);
}, 8 * 60 * 1000);
watchdog.unref();

try {
  for (const size of SIZES) {
    console.log(`\nTesting window size ${size.width}×${size.height}...`);
    await page.setViewportSize({ width: size.width, height: size.height });

    // --- Helper for the 4 shell assertions ---
    async function assertShellInvariants(screenName) {
      const metrics = await page.evaluate(() => ({
        scrollHeight: document.documentElement.scrollHeight,
        clientHeight: document.documentElement.clientHeight,
        innerHeight: window.innerHeight,
        scrollWidth: document.documentElement.scrollWidth,
        clientWidth: document.documentElement.clientWidth,
        innerWidth: window.innerWidth,
        scrollX: window.scrollX || document.documentElement.scrollLeft || 0,
        scrollY: window.scrollY || document.documentElement.scrollTop || 0,
      }));

      // 1. Page itself does not scroll vertically
      assert(
        metrics.scrollHeight <= metrics.innerHeight + 1,
        `[${screenName} ${size.width}×${size.height}] Page scrollHeight (${metrics.scrollHeight}) exceeds innerHeight + 1 (${metrics.innerHeight + 1})`
      );

      // 2. No horizontal page scroll
      assert(
        metrics.scrollWidth <= metrics.innerWidth + 1 && metrics.scrollX === 0,
        `[${screenName} ${size.width}×${size.height}] Horizontal page scroll detected: scrollWidth=${metrics.scrollWidth}, innerWidth=${metrics.innerWidth}, scrollX=${metrics.scrollX}`
      );

      // 3. .sidebar bounding box has top === 0 and height === innerHeight (±1)
      const sidebarBox = await page.locator('.sidebar').boundingBox();
      if (!sidebarBox) {
        assert(false, `[${screenName} ${size.width}×${size.height}] .sidebar element not found or invisible`);
      } else {
        assert(
          Math.abs(sidebarBox.y) <= 1,
          `[${screenName} ${size.width}×${size.height}] .sidebar top is ${sidebarBox.y}, expected 0 (±1)`
        );
        assert(
          Math.abs(sidebarBox.height - size.height) <= 1,
          `[${screenName} ${size.width}×${size.height}] .sidebar height is ${sidebarBox.height}, expected ${size.height} (±1)`
        );
      }

      // 4. All three nav links lie fully inside the viewport
      const navLinks = page.locator('nav a.nav-item');
      const navCount = await navLinks.count();
      assert(
        navCount === 3,
        `[${screenName} ${size.width}×${size.height}] Expected 3 nav links, found ${navCount}`
      );
      for (let i = 0; i < navCount; i++) {
        const linkBox = await navLinks.nth(i).boundingBox();
        if (!linkBox) {
          assert(false, `[${screenName} ${size.width}×${size.height}] Nav link #${i} has no bounding box`);
          continue;
        }
        const inside =
          linkBox.y >= 0 &&
          linkBox.y + linkBox.height <= size.height &&
          linkBox.x >= 0 &&
          linkBox.x + linkBox.width <= size.width;
        assert(
          inside,
          `[${screenName} ${size.width}×${size.height}] Nav link #${i} (${linkBox.x}, ${linkBox.y}, ${linkBox.width}×${linkBox.height}) not fully inside viewport (${size.width}×${size.height})`
        );
      }
    }

    // =========================================================================
    // 1. Develop
    // =========================================================================
    console.log(`  1. Develop screen at ${size.width}×${size.height}...`);
    await page.goto(`${base}/#/`, { waitUntil: 'networkidle' });
    const devFolderInput = page.locator('input[placeholder="/path/to/raw/photos"]');
    await devFolderInput.fill('/Volumes/Photos/RAW');
    await devFolderInput.press('Enter');

    await page.waitForSelector('.thumbnail-card', { timeout: 10000 });
    await page.locator('.thumbnail-card').first().click();
    await page.waitForTimeout(200);

    // Scroll .content to bottom
    await page.evaluate(() => {
      const content = document.querySelector('.content');
      if (content) content.scrollTop = content.scrollHeight;
    });
    await page.waitForTimeout(100);

    await assertShellInvariants('Develop');
    await page.screenshot({ path: join(OUT, `shell-develop-${size.width}.png`) });

    // =========================================================================
    // 2. Edit
    // =========================================================================
    console.log(`  2. Edit screen at ${size.width}×${size.height}...`);
    await page.goto(`${base}/#/edit`, { waitUntil: 'networkidle' });
    const editPathInput = page.locator('input[placeholder="/path/to/image.jpg"]');
    await editPathInput.fill('/Volumes/Photos/sample.jpg');
    await editPathInput.press('Enter');

    await page.waitForSelector('canvas[data-testid="edit-canvas"]', { timeout: 10000 });
    await page.waitForTimeout(200);

    // Expand every collapsible section inside the adjustments panel (do NOT click Choose…)
    while (true) {
      const collapsibles = page.locator('.adjustment-panel button[aria-expanded="false"]');
      const count = await collapsibles.count();
      let clickedOne = false;
      for (let i = 0; i < count; i++) {
        const btn = collapsibles.nth(i);
        const text = (await btn.textContent()) || '';
        if (text.includes('Choose')) continue;
        await btn.click();
        await page.waitForTimeout(50);
        clickedOne = true;
        break;
      }
      if (!clickedOne) break;
    }

    // Scroll .content and the panel's own scroll container to bottom
    await page.evaluate(() => {
      const content = document.querySelector('.content');
      if (content) content.scrollTop = content.scrollHeight;
      const panel = document.querySelector('.adjustment-panel');
      if (panel) panel.scrollTop = panel.scrollHeight;
    });
    await page.waitForTimeout(100);

    await assertShellInvariants('Edit');
    await page.screenshot({ path: join(OUT, `shell-edit-${size.width}.png`) });

    // =========================================================================
    // 3. Batch
    // =========================================================================
    console.log(`  3. Batch screen at ${size.width}×${size.height}...`);
    await page.goto(`${base}/#/batch`, { waitUntil: 'networkidle' });
    await page.waitForSelector('.bulk-lut-page', { timeout: 10000 });
    await page.waitForTimeout(200);

    // Scroll .content to bottom
    await page.evaluate(() => {
      const content = document.querySelector('.content');
      if (content) content.scrollTop = content.scrollHeight;
    });
    await page.waitForTimeout(100);

    await assertShellInvariants('Batch');
    await page.screenshot({ path: join(OUT, `shell-batch-${size.width}.png`) });

    // =========================================================================
    // 4. The rail
    // =========================================================================
    console.log(`  4. The rail at ${size.width}×${size.height}...`);
    const railSidebarBox = await page.locator('.sidebar').boundingBox();
    if (!railSidebarBox) {
      assert(false, `[Rail ${size.width}×${size.height}] .sidebar element not found`);
    } else {
      assert(
        Math.abs(railSidebarBox.width - 56) <= 1,
        `[Rail ${size.width}×${size.height}] .sidebar width is ${railSidebarBox.width}px, expected 56 ± 1 px`
      );
    }

    const railLinks = page.locator('nav a.nav-item');
    const expectedLabels = ['Develop', 'Edit', 'Batch'];
    const expectedHashes = ['#/', '#/edit', '#/batch'];

    for (let i = 0; i < 3; i++) {
      const link = railLinks.nth(i);
      const box = await link.boundingBox();
      if (!box) {
        assert(false, `[Rail ${size.width}×${size.height}] Nav link #${i} has no bounding box`);
      } else {
        assert(
          box.height >= 55.5,
          `[Rail ${size.width}×${size.height}] Nav link #${i} (${expectedLabels[i]}) height is ${box.height}px, expected >= 56px`
        );
      }
      const ariaLabel = await link.getAttribute('aria-label');
      assert(
        ariaLabel === expectedLabels[i],
        `[Rail ${size.width}×${size.height}] Nav link #${i} aria-label is "${ariaLabel}", expected "${expectedLabels[i]}"`
      );
    }

    // Test navigation by clicking each tab
    for (let i = 0; i < 3; i++) {
      const link = railLinks.nth(i);
      await link.click();
      await page.waitForTimeout(100);
      const currentHash = await page.evaluate(() => window.location.hash);
      assert(
        currentHash === expectedHashes[i],
        `[Rail ${size.width}×${size.height}] Clicking nav link #${i} gave hash "${currentHash}", expected "${expectedHashes[i]}"`
      );
    }
  }
} catch (err) {
  failures.push(`Unexpected error: ${err.message}\n${err.stack}`);
} finally {
  await browser.close();
  await server.close();
}

console.log(`\n${passedAssertions} assertions passed.`);

if (failures.length > 0) {
  console.error(`\ncheck:shell FAILED with ${failures.length} failure(s):`);
  for (const f of failures) {
    console.error(`- ${f}`);
  }
  process.exit(1);
} else {
  console.log('\ncheck:shell PASSED cleanly.');
}
