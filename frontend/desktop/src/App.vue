<script setup lang="ts">
/**
 * The desktop shell.
 *
 * **Nothing here needs the server.** Reading a card, checking it, copying it to
 * a folder and every tool tab call `core` directly on this machine; the one
 * command that made an HTTP request was the card handoff, and no screen calls
 * it any more (`docs/workflow-plan.md`).
 *
 * It used to carry a reachability probe and a `// SERVER UNREACHABLE //`
 * banner. Warning about a dependency that no longer exists is worse than
 * saying nothing: it trains somebody to ignore a warning that might one day
 * matter. The probe, the banner and the LINKED/LOCAL zone indicator went with
 * it — a zone that can never change is not information.
 */
import { onMounted, onUnmounted, ref } from 'vue';
import { sharedToolLinks, stepLabel } from '@ui/routes';
import { desktop, syncTimeline } from './api';
import { refreshRoots } from '@ui/useRoots';

/**
 * The sidebar reads as the workflow does. Ingest is step 01 because it is
 * where a card enters, and the shared tools number on from there; publishing
 * is the last step and is not offered here (`docs/workflow-plan.md`).
 */
const links = [
  { to: '/', label: 'Ingest', step: stepLabel(1) },
  ...sharedToolLinks,
];

/** The status bar's live clock (§5.8). */
const clock = ref('');
let ticking: number | undefined;
function tick() {
  clock.value = new Date().toTimeString().slice(0, 8);
}

/**
 * The timeline sync (`docs/timeline-sync-plan.md`).
 *
 * The Mac drives it: a NAS cannot open a connection to a laptop that is asleep
 * or on somebody else's network. **Geopositions only** — tracks, their fixes,
 * the decisions recorded about them, and deletions. Nothing about cards, shots,
 * sessions or publishes crosses between the two machines.
 */
const syncing = ref(false);
const syncNote = ref<string | null>(null);

async function sync(automatic: boolean) {
  if (syncing.value) return;
  syncing.value = true;
  try {
    const report = await syncTimeline();
    syncNote.value = report.summary;
    // The tab on screen asked before the sync ran, and a pulled track changes
    // what a picker and a map should show.
    if (report.pulled || report.deleted_here) refreshRoots();
  } catch (e) {
    // A sync that failed says so; an automatic one says it quietly, because the
    // NAS being off is the normal state of a laptop on a train, not a fault.
    const message = e instanceof Error ? e.message : String(e);
    syncNote.value = automatic ? 'server not reached' : message;
  } finally {
    syncing.value = false;
  }
}

onMounted(async () => {
  tick();
  ticking = window.setInterval(tick, 1000);

  // At startup, and only if the server answers its three-second probe: the
  // application opens whether or not the NAS is there, and never waits for it.
  const status = await desktop.serverStatus().catch(() => null);
  if (status?.reachable) await sync(true);
});
onUnmounted(() => window.clearInterval(ticking));
</script>

<template>
  <div class="shell crt-boot">
    <aside class="sidebar">
      <div class="brand">
        <span class="brand__name">PHOTOTOOLS</span>
        <span class="brand__sub">// INGEST</span>
      </div>

      <nav>
        <RouterLink v-for="link in links" :key="link.to" :to="link.to" class="nav-item">
          <span class="nav-item__step">{{ link.step }}</span>{{ link.label }}
        </RouterLink>
      </nav>

      <div class="spacer"></div>

      <div class="sync">
        <button type="button" class="ghost" :disabled="syncing" @click="sync(false)">
          {{ syncing ? 'Syncing…' : 'Sync timeline' }}
        </button>
        <p v-if="syncNote" class="sync__note" role="status">{{ syncNote }}</p>
        <p class="sync__what">Geopositions only</p>
      </div>
    </aside>

    <div class="main">
      <main class="content">
        <RouterView />
      </main>

      <footer class="statusbar">
        <span class="statusbar__mid">PHOTOTOOLS v0.1.0</span>
        <span class="statusbar__right">{{ clock }}</span>
      </footer>
    </div>
  </div>
</template>

<style scoped>
.shell {
  display: grid;
  grid-template-columns: 232px 1fr;
  min-height: 100vh;
}

/* --- sidebar (§6.4): the roll list pattern, applied to tools ------------- */

.sidebar {
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
  padding: var(--space-4) var(--space-3);
  border-right: var(--border-hair);
  background: var(--bg-elevated);
}

.sync {
  display: grid;
  gap: var(--space-2);
  padding: var(--space-2);
  border-top: var(--border-hair);
}
.sync__note,
.sync__what {
  font-family: var(--font-body);
  font-size: 11px;
  color: var(--text-muted);
  /* No glow: this is small type, where a glow smears the glyph. */
  text-shadow: none;
}
.sync__what {
  color: var(--text-disabled);
}

.brand {
  display: grid;
  gap: 2px;
  padding: var(--space-2) var(--space-2) var(--space-5);
}
.brand__name {
  font-family: var(--font-ui);
  font-weight: 500;
  font-size: 14px;
  letter-spacing: 0.08em;
  color: var(--text-heading);
}
.brand__sub {
  font-family: var(--font-label);
  font-size: 12px;
  letter-spacing: 0.1em;
  color: var(--text-disabled);
}

nav {
  display: grid;
  gap: 1px;
}

.nav-item {
  padding: var(--space-2) var(--space-3);
  text-decoration: none;
  font-family: var(--font-label);
  font-size: 13px;
  letter-spacing: 0.1em;
  text-transform: uppercase;
  color: var(--text-disabled);
  /* The active marker is a left border, so the label never moves as it
     activates — a shifting nav is the fastest way to feel cheap. */
  border-left: 2px solid transparent;
  transition: color var(--dur-fast) var(--ease);
}
.nav-item:hover {
  color: var(--text);
  background: var(--bg-panel);
}

/* Fixed width so the labels line up in a column rather than stepping in and
   out by a character. Dimmer than the label, including on the active item:
   the number is an index, the word is the thing. */
.nav-item__step {
  display: inline-block;
  width: 3ch;
  color: var(--text-disabled);
  text-shadow: none;
}
.nav-item.router-link-exact-active {
  color: var(--accent);
  border-left-color: var(--accent);
  background: var(--bg-panel);
  text-shadow: var(--glow-phosphor);
}

.spacer {
  flex: 1;
}


/* --- main column --------------------------------------------------------- */

.main {
  display: grid;
  grid-template-rows: 1fr auto;
  min-height: 100vh;
  min-width: 0;
}

.content {
  padding: var(--space-5) var(--space-6) var(--space-7);
  overflow-y: auto;
}


/* --- status bar (§5.8) ---------------------------------------------------- */

.statusbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-3);
  min-height: var(--status-h);
  padding: 0 var(--space-4);
  border-top: var(--border-hair);
  background: var(--bg-elevated);
  font-family: var(--font-ui);
  font-size: 12px;
  letter-spacing: 0.1em;
  color: var(--text-muted);
  white-space: nowrap;
}
</style>
