/**
 * Transport boundary check for Rollchrome.
 *
 * Three boundaries are checked:
 *
 * 1. **Transport.** Only `src/api.ts` may construct a transport, and no file
 *    may reach the network directly (`fetch`, `XMLHttpRequest`, `EventSource`,
 *    `axios`).
 * 2. **The host surface.** Anything imported from `@host/` must be `@host/api`.
 * 3. **No HTTP client.** No file imports `HttpApiClient` or imports a runtime
 *    value (as opposed to `import type`) from `@phototools/shared`.
 */
import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative, resolve } from 'node:path';

const ALLOWED = new Set(['src/api.ts']);
const FORBIDDEN = [
  { pattern: /\bfetch\s*\(/, what: 'fetch()' },
  { pattern: /\bXMLHttpRequest\b/, what: 'XMLHttpRequest' },
  { pattern: /\bnew\s+EventSource\b/, what: 'EventSource' },
  { pattern: /\baxios\b/, what: 'axios' },
];

/** The only module of this application shared files may import. */
const HOST_SURFACE = new Set(['@host/api']);
const HOST_IMPORT = /from\s+'(@host\/[^']+)'/g;

/** Match every import statement from `@phototools/shared`. */
const SHARED_IMPORT = /\bimport\s+([^;'"]*?)\s+from\s+['"]@phototools\/shared['"]/g;

/** Scanned roots: this application's `src/`, and the shared files it compiles. */
const ROOTS = [
  'src',
  resolve('..', 'shared', 'src', 'ui', 'useRoots.ts'),
  resolve('..', 'shared', 'src', 'ui', 'components', 'JobProgress.vue'),
  resolve('..', 'shared', 'src', 'ui', 'components', 'PathField.vue'),
  resolve('..', 'shared', 'src', 'ui', 'components', 'PathListField.vue'),
  resolve('..', 'shared', 'src', 'ui', 'components', 'FolderPicker.vue'),
];

function walk(target) {
  if (!statSync(target).isDirectory()) return [target];
  const out = [];
  for (const entry of readdirSync(target)) {
    const full = join(target, entry);
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

    if (/\bHttpApiClient\b/.test(source)) {
      offences.push(`${label}: references HttpApiClient`);
    }

    for (const [, clause] of source.matchAll(SHARED_IMPORT)) {
      if (!clause.trim().startsWith('type ') && !clause.trim().startsWith('type{')) {
        offences.push(
          `${label}: imports a value from @phototools/shared (only 'import type' is allowed)`,
        );
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
  console.error('Rollchrome transport check failed:');
  for (const offence of offences) console.error(`  - ${offence}`);
  process.exit(1);
}

console.log(`transport check: ${scanned} files, no view reaches the network directly and no HTTP client is imported`);
