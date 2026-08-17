/**
 * Phase 6 acceptance: no view imports `fetch` directly — all traffic goes
 * through the shared client.
 *
 * Two boundaries are checked:
 *
 * 1. **Transport.** Only `src/api.ts` and `src/auth.ts` (which delegates to the Firebase SDK) may construct a transport.
 *    Everything else goes through the `ApiClient` interface, so the two builds
 *    can swap transports without touching a single view.
 * 2. **The host surface.** The shared views are compiled into both front ends,
 *    so anything they import from `@host/` must exist in both. Only
 *    `@host/api` is guaranteed; reaching for anything else would couple the
 *    shared views to one application.
 *
 * The shared views are scanned as well as this package's own `src`, because
 * that is where most views now live.
 */
import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative, resolve } from 'node:path';

const ALLOWED = new Set(['src/api.ts', 'src/auth.ts']);
const FORBIDDEN = [
  { pattern: /\bfetch\s*\(/, what: 'fetch()' },
  { pattern: /\bXMLHttpRequest\b/, what: 'XMLHttpRequest' },
  { pattern: /\bnew\s+EventSource\b/, what: 'EventSource' },
  { pattern: /\baxios\b/, what: 'axios' },
];

/** The only module of this application the shared views may import. */
const HOST_SURFACE = new Set(['@host/api']);
const HOST_IMPORT = /from\s+'(@host\/[^']+)'/g;

/** Scanned roots: this application, and the shared views it compiles in. */
const ROOTS = ['src', resolve('..', 'shared', 'src', 'ui')];

function walk(dir) {
  const out = [];
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) out.push(...walk(full));
    else if (/\.(ts|vue)$/.test(entry)) out.push(full);
  }
  return out;
}

const offences = [];
let scanned = 0;

for (const root of ROOTS) {
  // A root that has moved must fail the check rather than silently pass it.
  if (!existsSync(root)) {
    offences.push(`${root}: scan root does not exist`);
    continue;
  }

  for (const file of walk(root)) {
    scanned += 1;
    const label = relative('.', file).replace(/\\/g, '/');
    const source = readFileSync(file, 'utf8');

    if (!ALLOWED.has(label)) {
      for (const { pattern, what } of FORBIDDEN) {
        if (pattern.test(source)) offences.push(`${label}: uses ${what}`);
      }
    }

    for (const [, specifier] of source.matchAll(HOST_IMPORT)) {
      if (!HOST_SURFACE.has(specifier)) {
        offences.push(`${label}: imports ${specifier}, which only one front end provides`);
      }
    }
  }
}

if (offences.length) {
  console.error('Views must use the shared ApiClient, not a transport directly:');
  for (const offence of offences) console.error(`  - ${offence}`);
  process.exit(1);
}

console.log(`transport check: ${scanned} files, no view reaches the network directly`);
