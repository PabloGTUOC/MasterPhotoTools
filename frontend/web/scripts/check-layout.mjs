/**
 * Phase 6 acceptance: layout verified at 390 px width.
 *
 * Serves the production build and drives a real browser at an iPhone-class
 * viewport, asserting that no route scrolls sideways and that every interactive
 * control is a comfortable touch target. Screenshots land in `layout-proof/` for
 * a human to look at.
 */
import { createServer } from 'node:http';
import { readFileSync, existsSync, mkdirSync, readdirSync } from 'node:fs';
import { extname, join, normalize } from 'node:path';
import { chromium } from 'playwright';

/**
 * The environment pre-installs a Chromium build that may not match the version
 * this Playwright pins, and downloading another is not on. Point at whichever
 * pre-installed binary is present, and fall back to Playwright's own lookup.
 */
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

const WIDTH = 390;
const HEIGHT = 844;
const DIST = 'dist';
const OUT = 'layout-proof';

const TYPES = {
  '.html': 'text/html',
  '.js': 'text/javascript',
  '.css': 'text/css',
  '.svg': 'image/svg+xml',
  '.png': 'image/png',
  '.json': 'application/json',
};

const ROUTES = [
  '/', '/library', '/publish', '/dates', '/rename',
  '/split', '/contact-sheet', '/transform', '/border', '/tiff-to-jpeg',
];

if (!existsSync(DIST)) {
  console.error(`No ${DIST}/ — run \`npm run build\` first.`);
  process.exit(1);
}
mkdirSync(OUT, { recursive: true });

// A single-page app: unknown paths fall back to index.html.
const server = createServer((req, res) => {
  const url = (req.url ?? '/').split('?')[0];
  const candidate = join(DIST, normalize(url));
  const file = existsSync(candidate) && extname(candidate) ? candidate : join(DIST, 'index.html');
  res.writeHead(200, { 'Content-Type': TYPES[extname(file)] ?? 'application/octet-stream' });
  res.end(readFileSync(file));
});

await new Promise((resolve) => server.listen(0, resolve));
const base = `http://127.0.0.1:${server.address().port}`;

const browser = await chromium.launch({ executablePath: preinstalledChromium() });
const page = await browser.newPage({
  viewport: { width: WIDTH, height: HEIGHT },
  deviceScaleFactor: 2,
  isMobile: true,
  hasTouch: true,
});

const failures = [];

for (const route of ROUTES) {
  await page.goto(`${base}${route}`, { waitUntil: 'networkidle' });
  await page.waitForTimeout(150);

  const report = await page.evaluate((width) => {
    /** True if this element, or an ancestor, scrolls horizontally by design. */
    const insideScroller = (el) => {
      for (let node = el; node && node !== document.body; node = node.parentElement) {
        const overflow = getComputedStyle(node).overflowX;
        if (overflow === 'auto' || overflow === 'scroll') return true;
      }
      return false;
    };

    const overflowing = [];
    for (const el of document.querySelectorAll('body *')) {
      const rect = el.getBoundingClientRect();
      if (rect.width === 0 && rect.height === 0) continue;
      // Content inside a deliberately scrollable strip is not page overflow;
      // the assertion that the *page* does not scroll sideways still stands.
      if (insideScroller(el)) continue;
      if (rect.right > width + 1) {
        overflowing.push(`${el.tagName.toLowerCase()}.${el.className || '(no class)'}`);
      }
    }

    const small = [];
    for (const el of document.querySelectorAll('button, a, input, select, textarea')) {
      const rect = el.getBoundingClientRect();
      if (rect.width === 0 && rect.height === 0) continue;

      // A checkbox or radio is a small glyph; what the finger lands on is its
      // label, so that is what must be big enough.
      const isTick = el.tagName === 'INPUT' && (el.type === 'checkbox' || el.type === 'radio');
      const target = isTick ? (el.closest('label') ?? el) : el;
      const height = target.getBoundingClientRect().height;

      if (height < 40) {
        small.push(`${el.tagName.toLowerCase()}${isTick ? `[${el.type}]` : ''} ${height.toFixed(0)}px`);
      }
    }

    return {
      documentWidth: document.documentElement.scrollWidth,
      overflowing: overflowing.slice(0, 5),
      small: small.slice(0, 5),
    };
  }, WIDTH);

  const name = route === '/' ? 'home' : route.slice(1);
  await page.screenshot({ path: join(OUT, `${name}.png`), fullPage: true });

  if (report.documentWidth > WIDTH) {
    failures.push(`${route}: page scrolls sideways (${report.documentWidth}px > ${WIDTH}px)`);
  }
  if (report.overflowing.length) {
    failures.push(`${route}: element past the viewport — ${report.overflowing.join(', ')}`);
  }
  if (report.small.length) {
    failures.push(`${route}: touch target under 40px — ${report.small.join(', ')}`);
  }

  console.log(
    `${route.padEnd(16)} width=${report.documentWidth}px ` +
      `${report.overflowing.length === 0 && report.small.length === 0 ? 'ok' : 'PROBLEM'}`,
  );
}

await browser.close();
server.close();

if (failures.length) {
  console.error(`\nLayout problems at ${WIDTH}px:`);
  for (const failure of failures) console.error(`  - ${failure}`);
  process.exit(1);
}
console.log(`\nAll ${ROUTES.length} routes clean at ${WIDTH}x${HEIGHT}. Screenshots in ${OUT}/`);
