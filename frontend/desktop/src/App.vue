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
import { sharedToolLinks } from '@ui/routes';

const links = [
  { to: '/', label: 'Ingest' },
  ...sharedToolLinks,
];

/** The status bar's live clock (§5.8). */
const clock = ref('');
let ticking: number | undefined;
function tick() {
  clock.value = new Date().toTimeString().slice(0, 8);
}

onMounted(() => {
  tick();
  ticking = window.setInterval(tick, 1000);
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
          {{ link.label }}
        </RouterLink>
      </nav>

      <div class="spacer"></div>
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
