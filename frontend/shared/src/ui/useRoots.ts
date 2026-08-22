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
 * cannot have changed.
 */
import { ref, type Ref } from 'vue';
import { api } from '@host/api';

const roots = ref<string[]>([]);
const failure = ref<string | null>(null);
let asked: Promise<void> | null = null;

export function useRoots(): { roots: Ref<string[]>; failure: Ref<string | null> } {
  asked ??= api
    .roots()
    .then((r) => {
      roots.value = r;
    })
    .catch((e: unknown) => {
      // Not fatal: the path can still be typed. The picker says what it knows.
      failure.value = e instanceof Error ? e.message : String(e);
      // And not permanent: forget the attempt so the next view to ask tries
      // again. Caching a failure for the life of the page meant one refused
      // request — during sign-in, or with the server briefly down — left every
      // picker empty until a reload.
      asked = null;
    });

  return { roots, failure };
}

/** Test seam: forget the cached answer. */
export function resetRoots() {
  asked = null;
  roots.value = [];
  failure.value = null;
}
