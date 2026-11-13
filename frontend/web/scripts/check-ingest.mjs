/**
 * Phase 13 acceptance, in a real browser.
 *
 * One criterion, a claim about behaviour rather than about code. Two more — bulk-approving resizes
 * and the publish gate — went with remediation and Google publishing (RS-1, 2026-11-14).
 *
 * 1. **A 400-shot session renders and stays responsive.** "Responsive" is a
 *    measurement, not an opinion, so the grid is built on its own and four
 *    hundred shots are put through it: first paint, filtering and scrolling are
 *    all timed, every shot is reachable, and the DOM is checked to hold a
 *    screenful rather than a card's worth.
 *
 * Screenshots land in `layout-proof/` for a human to look at.
 */
import { existsSync, mkdirSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath, URL } from 'node:url';
import { createServer } from 'vite';
import vue from '@vitejs/plugin-vue';
import { chromium } from 'playwright';

/** As `check-layout.mjs`: use whichever Chromium the environment pre-installed. */
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

const SHOTS = 400;
/** A filter must repaint within this. Well under the 100 ms that reads as instant. */
const FILTER_BUDGET_MS = 100;
/** First paint of a full card, measured against a warm dev server. */
const RENDER_BUDGET_MS = 700;
/** A scrolled frame. Sixteen milliseconds is one frame at 60 Hz; allow two. */
const SCROLL_BUDGET_MS = 33;
const OUT = 'layout-proof';

const ui = fileURLToPath(new URL('../../shared/src/ui', import.meta.url));
const harness = fileURLToPath(new URL('./grid-harness', import.meta.url));

mkdirSync(OUT, { recursive: true });

const server = await createServer({
  root: harness,
  configFile: false,
  plugins: [vue()],
  resolve: {
    dedupe: ['vue'],
    alias: [{ find: /^@ui\//, replacement: `${ui}/` }],
  },
  server: { fs: { allow: [harness, ui, fileURLToPath(new URL('../..', import.meta.url))] } },
  logLevel: 'warn',
});
await server.listen();

// Ask the socket where it actually landed rather than assuming. Vite binds to
// `localhost`, which Node resolves in the host's DNS order — on macOS that is
// `::1` ahead of `127.0.0.1`, so a hardcoded IPv4 URL is refused outright. The
// configured port is the requested one, not the bound one, and differs whenever
// 5173 is already taken.
const bound = server.httpServer.address();
const host = bound.family === 'IPv6' ? `[${bound.address}]` : bound.address;
const base = `http://${host}:${bound.port}`;

const browser = await chromium.launch({ executablePath: preinstalledChromium() });
const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });

const failures = [];
const note = (message) => console.log(`  ${message}`);

try {
  // Warm the dev server first. Vite compiles the component on demand, and a
  // cold compile is several hundred milliseconds of tooling that has nothing to
  // do with whether the grid is fast.
  await page.goto(`${base}/?shots=1`, { waitUntil: 'networkidle' });

  const started = Date.now();
  await page.goto(`${base}/?shots=${SHOTS}`, { waitUntil: 'networkidle' });
  await page.waitForSelector('[data-testid="shot-row"]');
  const renderMs = Date.now() - started;

  note(`first paint with ${SHOTS} shots: ${renderMs} ms`);
  if (renderMs > RENDER_BUDGET_MS) {
    failures.push(`first paint took ${renderMs} ms, over the ${RENDER_BUDGET_MS} ms budget`);
  }

  // (2) A screenful, not a card's worth.
  const built = await page.locator('[data-testid="shot-row"]').count();
  note(`rows built into the DOM: ${built} of ${SHOTS}`);
  if (built >= SHOTS) {
    failures.push(
      `${built} rows are in the DOM; the grid is meant to window, so a filter ` +
        `change would tear down and rebuild all ${SHOTS}`,
    );
  }
  if (built < 5) {
    failures.push(`only ${built} rows rendered, which is not a screenful`);
  }

  // The chips have to say what the verdict says. A grid that renders four
  // hundred rows quickly and colours them all the same is not a review screen.
  const chips = await page.evaluate(() => {
    const rowFor = (stem) =>
      [...document.querySelectorAll('[data-testid="shot-row"]')].find(
        (row) => row.querySelector('.stem')?.textContent?.trim() === stem,
      );
    const statuses = (stem) =>
      [...(rowFor(stem)?.querySelectorAll('.chip') ?? [])].map((chip) => [
        // The rule name is the last text node; the mark and the screen-reader
        // word come before it.
        chip.lastChild.textContent.trim(),
        chip.dataset.status,
        chip.querySelector('.mark')?.textContent?.trim(),
      ]);
    return { failing: statuses('IMG_0000'), passing: statuses('IMG_0007') };
  });

  note(`IMG_0000 chips: ${JSON.stringify(chips.failing)}`);
  note(`IMG_0007 chips: ${JSON.stringify(chips.passing)}`);

  const statusOf = (list, label) => list.find(([name]) => name === label)?.[1];
  const markOf = (list, label) => list.find(([name]) => name === label)?.[2];

  // Colour must not be the only carrier of the verdict.
  if (markOf(chips.failing, 'resolution') === markOf(chips.passing, 'resolution')) {
    failures.push(
      'passing and failing chips carry the same mark, so the verdict is in the ' +
        'colour alone — which is not a distinction everybody can make',
    );
  }
  if (statusOf(chips.failing, 'capture date') !== 'fail') {
    failures.push('a shot with no capture date is not shown as failing that check');
  }
  if (statusOf(chips.failing, 'resolution') !== 'fail') {
    failures.push('a 24 MP shot against a 10 MP ceiling is not shown as failing');
  }
  if (statusOf(chips.passing, 'resolution') !== 'pass') {
    failures.push('a 6 MP shot is not shown as passing the resolution check');
  }
  if (statusOf(chips.passing, 'file size') !== 'pass') {
    failures.push('a 3 MB shot is not shown as passing the size check');
  }

  // (1) The last shot is reachable.
  const viewport = page.locator('[data-testid="shot-viewport"]');
  await viewport.evaluate((element) => element.scrollTo(0, element.scrollHeight));
  await page.waitForTimeout(120);
  const last = `IMG_${String(SHOTS - 1).padStart(4, '0')}`;
  const reachedEnd = await page.locator(`text=${last}`).count();
  note(`last shot (${last}) reachable: ${reachedEnd > 0}`);
  if (reachedEnd === 0) {
    failures.push(`scrolling to the end did not reach ${last}`);
  }

  await viewport.evaluate((element) => element.scrollTo(0, 0));
  await page.waitForTimeout(80);

  // (3) Filtering to one failure class, as a person reading the status does.
  const filterMs = await page.evaluate(async () => {
    const button = document.querySelector('[data-testid="filter-pixels"]');
    const start = performance.now();
    button.click();
    // Let Vue flush and the browser paint before stopping the clock.
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    return performance.now() - start;
  });

  const afterFilter = await page.locator('[data-testid="shot-row"]').count();
  note(`filter to one failure class: ${filterMs.toFixed(1)} ms, ${afterFilter} rows built`);
  if (filterMs > FILTER_BUDGET_MS) {
    failures.push(
      `filtering took ${filterMs.toFixed(1)} ms, over the ${FILTER_BUDGET_MS} ms budget`,
    );
  }
  if (afterFilter === 0) {
    failures.push('filtering to too_many_pixels left nothing on screen');
  }

  // Scrolling is the other thing "responsive" has to mean for a long list.
  await page.evaluate(() =>
    document.querySelector('[data-testid="filter-none"]').click(),
  );
  await page.waitForTimeout(80);

  const scrollMs = await page.evaluate(async () => {
    const element = document.querySelector('[data-testid="shot-viewport"]');
    const start = performance.now();
    // Ten frames of scrolling, as a flick would produce.
    for (let i = 1; i <= 10; i += 1) {
      element.scrollTop = i * 400;
      await new Promise((resolve) => requestAnimationFrame(resolve));
    }
    await new Promise((resolve) => requestAnimationFrame(resolve));
    return (performance.now() - start) / 10;
  });

  note(`scrolling: ${scrollMs.toFixed(1)} ms per frame`);
  if (scrollMs > SCROLL_BUDGET_MS) {
    failures.push(
      `scrolling cost ${scrollMs.toFixed(1)} ms a frame, over the ${SCROLL_BUDGET_MS} ms budget`,
    );
  }

  await page.evaluate(() =>
    document.querySelector('[data-testid="shot-viewport"]').scrollTo(0, 0),
  );
  await page.waitForTimeout(80);
  await page.screenshot({ path: join(OUT, 'shot-grid-400.png'), fullPage: false });
} finally {
  await browser.close();
  await server.close();
}

if (failures.length) {
  console.error('\ningest check FAILED');
  for (const failure of failures) console.error(`  - ${failure}`);
  process.exit(1);
}

console.log(`\ningest check: ${SHOTS} shots render responsively`);
