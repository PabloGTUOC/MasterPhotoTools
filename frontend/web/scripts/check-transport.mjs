/**
 * Phase 6 acceptance: no view imports `fetch` directly — all traffic goes
 * through the shared client.
 *
 * `src/api.ts` is the one module allowed to construct a transport, and
 * `src/auth.ts` delegates to the Firebase SDK. Everything else must go through
 * the `ApiClient` interface, so the desktop build can swap the transport
 * without touching a single view.
 */
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';

const ALLOWED = new Set(['src/api.ts', 'src/auth.ts']);
const FORBIDDEN = [
  { pattern: /\bfetch\s*\(/, what: 'fetch()' },
  { pattern: /\bXMLHttpRequest\b/, what: 'XMLHttpRequest' },
  { pattern: /\bnew\s+EventSource\b/, what: 'EventSource' },
  { pattern: /\baxios\b/, what: 'axios' },
];

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
for (const file of walk('src')) {
  const relative = file.replace(/\\/g, '/');
  if (ALLOWED.has(relative)) continue;

  const source = readFileSync(file, 'utf8');
  for (const { pattern, what } of FORBIDDEN) {
    if (pattern.test(source)) offences.push(`${relative}: uses ${what}`);
  }
}

if (offences.length) {
  console.error('Views must use the shared ApiClient, not a transport directly:');
  for (const offence of offences) console.error(`  - ${offence}`);
  process.exit(1);
}

console.log('transport check: no view reaches the network directly');
