/**
 * The directories this build may browse.
 *
 * View-side rather than component-side: it reaches `@host/api`, so it obeys the
 * same rule `views/` does — only `ApiClient` methods both transports implement.
 * `roots()` is one, and the two answers deliberately differ: the desktop's are
 * on the Mac, the server's are inside its container.
 *
 * Cached for the life of the page. The roots come from configuration read at
 * startup, so re-asking on every view change is a round trip for an answer that
 * cannot have changed — except when who is asking changes, which is what
 * `refreshRoots` is for.
 */
import { ref, type Ref } from 'vue';
import { api } from '@host/api';

const roots = ref<string[]>([]);
const failure = ref<string | null>(null);
let asked: Promise<void> | null = null;

function ask(): Promise<void> {
  return api
    .roots()
    .then((r) => {
      roots.value = r;
      failure.value = null;
    })
    .catch((e: unknown) => {
      // Not fatal: the path can still be typed. The picker says what it knows —
      // and it has to be told this, or an empty list reads as "none configured"
      // and sends somebody to edit ROOTS on a server where ROOTS is fine.
      roots.value = [];
      failure.value = describe(e);
      // And not permanent: forget the attempt so the next view to ask tries
      // again. Caching a failure for the life of the page meant one refused
      // request — during sign-in, or with the server briefly down — left every
      // picker empty until a reload.
      asked = null;
    });
}

/**
 * Why the roots could not be had, in words for the person at the picker.
 *
 * `missing_token` is the one worth rewording: the server's message is about a
 * header, and what it means at a picker is that nobody has signed in yet.
 * Matched on the code rather than with `instanceof ApiError`: the shared views
 * and the application can resolve the client package to two copies, and an
 * `instanceof` across them is quietly false.
 */
function describe(e: unknown): string {
  if ((e as { code?: unknown } | null)?.code === 'missing_token') {
    return 'you are not signed in. Sign in, and the folders appear.';
  }
  return e instanceof Error ? e.message : String(e);
}

export function useRoots(): { roots: Ref<string[]>; failure: Ref<string | null> } {
  asked ??= ask();
  return { roots, failure };
}

/**
 * Ask again, now.
 *
 * For when who is asking has changed. A view that asked before sign-in was
 * refused, and its picker stayed empty after it — the failure is forgotten, but
 * nothing re-asks until another view mounts, so the tab the person is looking
 * at never found out.
 */
export function refreshRoots(): void {
  asked = ask();
}

/** Test seam: forget the cached answer. */
export function resetRoots() {
  asked = null;
  roots.value = [];
  failure.value = null;
}
