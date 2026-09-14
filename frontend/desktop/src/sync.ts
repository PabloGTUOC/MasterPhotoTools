/**
 * The timeline sync, as the whole application sees it
 * (`docs/timeline-sync-plan.md`).
 *
 * Module-level state rather than a component's, because two places act on the
 * same sync: the shell runs one at startup, and the Timeline tab has the button
 * and shows what happened. A second copy of "what did the last sync say?" would
 * be two answers to one question.
 *
 * **Desktop-only, and permanently so.** The server cannot sync with itself, and
 * a method one transport has to throw for is worse than one the type system
 * never offered (`frontend/shared/src/ui/README.md`).
 */
import { ref } from 'vue';
import { refreshRoots } from '@ui/useRoots';
import { syncTimeline } from './api';

export const syncing = ref(false);

/** What the last sync said, in the words it said them. */
export const lastSync = ref<string | null>(null);

/** True when the last attempt failed, for the screen to colour it. */
export const lastFailed = ref(false);

/**
 * Run one sync.
 *
 * `automatic` softens the report of a failure: a NAS that is off is the normal
 * condition of a laptop on a train, and a startup attempt that says
 * "server not reached" is telling the truth without raising an alarm. A sync
 * somebody asked for says exactly what went wrong.
 */
export async function runSync(automatic = false): Promise<void> {
  if (syncing.value) return;
  syncing.value = true;
  try {
    const report = await syncTimeline();
    lastSync.value = report.summary;
    lastFailed.value = false;
    // The tab on screen asked before the sync ran, and a pulled track changes
    // what a map should show.
    if (report.pulled || report.deleted_here) refreshRoots();
  } catch (e) {
    const message = e instanceof Error ? e.message : String(e);
    lastSync.value = automatic ? 'server not reached' : message;
    lastFailed.value = true;
  } finally {
    syncing.value = false;
  }
}
