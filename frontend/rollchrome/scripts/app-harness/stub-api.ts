// The three screens' stand-ins behind one object, preferring the one for the current route, so the whole
// application can be mounted and measured (UI-1).
import { desktop as dev } from '../develop-harness/stub-api';
import { desktop as edit } from '../edit-harness/stub-api';
import { desktop as bulk } from '../bulk-lut-harness/stub-api';

function order(): unknown[] {
  const h = window.location.hash;
  if (h.startsWith('#/edit')) return [edit, dev, bulk];
  if (h.startsWith('#/batch')) return [bulk, edit, dev];
  return [dev, edit, bulk];
}

export const desktop: any = new Proxy({}, {
  get(_t, p) {
    for (const s of order() as Record<string | symbol, unknown>[]) {
      const v = s[p];
      if (v !== undefined) return typeof v === 'function' ? (v as Function).bind(s) : v;
    }
    return undefined;
  },
});
export const api = desktop;
