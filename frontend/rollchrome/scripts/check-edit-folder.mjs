/**
 * UI-8 acceptance check: verify Edit opens a folder, lists its photographs on a filmstrip,
 * marks edited ones, handles click and arrow navigation, maintains concurrency limit of 2,
 * and preserves no-scroll layout discipline.
 *
 * Runs on the whole-application harness at 1280×800 and 1024×700.
 *
 * Steps:
 * 1. Fill the field with /Volumes/Photos and press Enter.
 *    - Three cards, in order, inside the viewport, in one row.
 *    - sample.jpg is active and opened: the stand-in's openPreview was called with /Volumes/Photos/sample.jpg.
 *    - second.jpg's card shows "edited".
 * 2. Click third.tif: openPreview was called with its path, once, and its card is is-last-clicked.
 * 3. Focus the strip and press ArrowLeft: second.jpg opens.
 * 4. Fill the field with /Volumes/Photos/second.jpg: the same three cards, second.jpg active.
 * 5. Throughout, the stand-in saw at most 2 concurrent thumbnail calls, and .content's scrollHeight <= clientHeight + 1.
 *
 * Prints "N assertions passed" and writes layout-proof/edit-folder-<width>.png.
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

const page = await browser.newPage();
page.setDefaultTimeout(15000);

let passedAssertions = 0;
const failures = [];

function assert(condition, message) {
  if (condition) {
    passedAssertions++;
  } else {
    failures.push(message);
    console.error(`  FAIL: ${message}`);
  }
}

const SIZES = [
  { width: 1280, height: 800 },
  { width: 1024, height: 700 },
];

const watchdog = setTimeout(() => {
  console.error('\ncheck:edit-folder FAILED: watchdog timeout after 5 minutes');
  process.exit(1);
}, 5 * 60 * 1000);
watchdog.unref();

try {
  for (const size of SIZES) {
    console.log(`\nTesting window size ${size.width}×${size.height}...`);
    await page.setViewportSize({ width: size.width, height: size.height });

    await page.goto(`${base}/#/edit`, { waitUntil: 'networkidle' });

    // Reset stub state before assertions
    await page.evaluate(() => {
      if (window.__STUB__) {
        window.__STUB__.openPreviewCalls = [];
        window.__STUB__.activeThumbnailRequests = 0;
        window.__STUB__.maxConcurrentThumbnailRequests = 0;
        window.__STUB__.thumbnailRequests = [];
      }
    });

    const field = page.locator('input[placeholder="/path/to/image.jpg"]');
    await field.waitFor({ state: 'visible' });

    // 1. Fill the field with /Volumes/Photos and press Enter
    await field.fill('/Volumes/Photos');
    await field.press('Enter');

    // Wait for the thumbnail cards to appear
    await page.waitForSelector('[data-testid="thumbnail-item"]', { timeout: 10000 });
    const cards = page.locator('[data-testid="thumbnail-item"]');
    await cards.first().waitFor({ state: 'visible' });

    // Wait for initial preview to load
    await page.waitForFunction(() => {
      const calls = window.__STUB__?.openPreviewCalls ?? [];
      return calls.includes('/Volumes/Photos/sample.jpg');
    }, { timeout: 10000 });

    const cardCount = await cards.count();
    assert(cardCount === 3, `[${size.width}×${size.height}] Expected 3 thumbnail cards, got ${cardCount}`);

    const cardData = await page.evaluate(() => {
      const items = Array.from(document.querySelectorAll('[data-testid="thumbnail-item"]'));
      return items.map((el) => {
        const rect = el.getBoundingClientRect();
        return {
          filename: el.getAttribute('data-filename') ?? '',
          active: el.classList.contains('active'),
          lastClicked: el.classList.contains('is-last-clicked'),
          caption: el.querySelector('.thumb-look')?.textContent?.trim() ?? '',
          rect: {
            top: rect.top,
            left: rect.left,
            right: rect.right,
            bottom: rect.bottom,
            width: rect.width,
            height: rect.height,
          },
        };
      });
    });

    // Check card order: sample.jpg, second.jpg, third.tif
    const names = cardData.map((c) => c.filename);
    assert(
      names[0] === 'sample.jpg' && names[1] === 'second.jpg' && names[2] === 'third.tif',
      `[${size.width}×${size.height}] Cards order expected sample.jpg, second.jpg, third.tif; got ${names.join(', ')}`
    );

    // Cards are inside viewport and in one row
    for (const c of cardData) {
      assert(
        c.rect.left >= 0 &&
        c.rect.top >= 0 &&
        c.rect.right <= size.width &&
        c.rect.bottom <= size.height,
        `[${size.width}×${size.height}] Card ${c.filename} is inside viewport (${JSON.stringify(c.rect)})`
      );
    }
    const topDiff = Math.max(...cardData.map((c) => c.rect.top)) - Math.min(...cardData.map((c) => c.rect.top));
    assert(
      topDiff <= 2,
      `[${size.width}×${size.height}] Cards are in one row (top difference ${topDiff} <= 2)`
    );

    // sample.jpg is active and opened
    assert(
      cardData[0].lastClicked,
      `[${size.width}×${size.height}] sample.jpg card has is-last-clicked`
    );

    // second.jpg's card shows "edited"
    assert(
      cardData[1].caption.includes('edited'),
      `[${size.width}×${size.height}] second.jpg card caption is "edited", got "${cardData[1].caption}"`
    );

    // 2. Click third.tif: openPreview was called with its path, and its card is is-last-clicked
    const thirdCard = page.locator('[data-filename="third.tif"]');
    await thirdCard.click();

    await page.waitForFunction(() => {
      const calls = window.__STUB__?.openPreviewCalls ?? [];
      return calls.includes('/Volumes/Photos/third.tif');
    }, { timeout: 10000 });

    const thirdCardState = await page.evaluate(() => {
      const el = document.querySelector('[data-filename="third.tif"]');
      return {
        lastClicked: el?.classList.contains('is-last-clicked') ?? false,
      };
    });
    assert(
      thirdCardState.lastClicked,
      `[${size.width}×${size.height}] third.tif card has is-last-clicked after click`
    );

    // A click opens the photograph once. Opening sets the path field, and the field's watcher must not
    // open it a second time: for a RAW that is a second full development (UI-8 review).
    await page.waitForTimeout(300);
    const thirdOpens = await page.evaluate(
      () => (window.__STUB__?.openPreviewCalls ?? []).filter((p) => p === '/Volumes/Photos/third.tif').length
    );
    assert(
      thirdOpens === 1,
      `[${size.width}×${size.height}] Clicking third.tif opened it ${thirdOpens} times, expected once`
    );

    // 3. Focus the strip and press ArrowLeft: second.jpg opens
    const filmstrip = page.locator('[data-testid="thumbnail-grid"]');
    await filmstrip.focus();
    await page.keyboard.press('ArrowLeft');

    await page.waitForFunction(() => {
      const calls = window.__STUB__?.openPreviewCalls ?? [];
      return calls[calls.length - 1] === '/Volumes/Photos/second.jpg';
    }, { timeout: 10000 });

    const secondCardState = await page.evaluate(() => {
      const el = document.querySelector('[data-filename="second.jpg"]');
      return {
        lastClicked: el?.classList.contains('is-last-clicked') ?? false,
      };
    });
    assert(
      secondCardState.lastClicked,
      `[${size.width}×${size.height}] second.jpg card has is-last-clicked after ArrowLeft`
    );

    // 4. Fill the field with /Volumes/Photos/second.jpg: the same three cards, second.jpg active
    await field.fill('/Volumes/Photos/second.jpg');
    await field.press('Enter');
    await page.waitForTimeout(200);

    const step4Cards = await page.evaluate(() => {
      const items = Array.from(document.querySelectorAll('[data-testid="thumbnail-item"]'));
      return {
        filenames: items.map((el) => el.getAttribute('data-filename') ?? ''),
        secondActive: items[1]?.classList.contains('is-last-clicked') ?? false,
      };
    });
    assert(
      step4Cards.filenames.length === 3 &&
      step4Cards.filenames[0] === 'sample.jpg' &&
      step4Cards.filenames[1] === 'second.jpg' &&
      step4Cards.filenames[2] === 'third.tif',
      `[${size.width}×${size.height}] The same three cards remain after entering photograph path`
    );
    assert(
      step4Cards.secondActive,
      `[${size.width}×${size.height}] second.jpg is active after entering its path`
    );

    // 5. Throughout, the stand-in saw at most 2 concurrent thumbnail calls, and .content's scrollHeight <= clientHeight + 1
    const finalMetrics = await page.evaluate(() => {
      const content = document.querySelector('.content');
      return {
        maxConcurrent: window.__STUB__?.maxConcurrentThumbnailRequests ?? 0,
        contentScrollHeight: content?.scrollHeight ?? 0,
        contentClientHeight: content?.clientHeight ?? 0,
      };
    });

    assert(
      finalMetrics.maxConcurrent <= 2,
      `[${size.width}×${size.height}] Stand-in saw at most 2 concurrent thumbnail calls (saw ${finalMetrics.maxConcurrent})`
    );
    assert(
      finalMetrics.maxConcurrent > 0,
      `[${size.width}×${size.height}] Stand-in saw thumbnail requests (saw ${finalMetrics.maxConcurrent})`
    );
    assert(
      finalMetrics.contentScrollHeight <= finalMetrics.contentClientHeight + 1,
      `[${size.width}×${size.height}] .content scrollHeight (${finalMetrics.contentScrollHeight}) <= clientHeight + 1 (${finalMetrics.contentClientHeight + 1})`
    );

    // Write screenshot
    await page.screenshot({ path: join(OUT, `edit-folder-${size.width}.png`) });
    console.log(`Wrote screenshot to layout-proof/edit-folder-${size.width}.png`);
  }

  if (failures.length > 0) {
    console.error(`\n${failures.length} assertion(s) failed.`);
    process.exit(1);
  } else {
    console.log(`\n${passedAssertions} assertions passed`);
  }
} finally {
  await browser.close();
  await server.close();
}
