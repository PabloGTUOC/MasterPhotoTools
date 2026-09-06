<script setup lang="ts">
/**
 * The desktop shell.
 *
 * Task 6 — graceful degradation: the server's reachability is shown, and
 * server-backed features disable with a clear indicator. Nothing local breaks
 * when the NAS is off.
 */
import { computed, onMounted, onUnmounted, ref } from 'vue';
import { desktop, type ServerStatus } from './api';

const links = [
  { to: '/', label: 'Ingest' },
  { to: '/dates', label: 'Dates' },
  { to: '/rename', label: 'Rename' },
  { to: '/geotag', label: 'Geotag' },
  { to: '/split', label: 'Split' },
  { to: '/contact-sheet', label: 'Sheet' },
  { to: '/transform', label: 'Transform' },
  { to: '/border', label: 'Border' },
  { to: '/tiff-to-jpeg', label: 'TIFF' },
];

const server = ref<ServerStatus | null>(null);
let timer: number | undefined;

/** The status bar's live clock (§5.8). */
const clock = ref('');
let ticking: number | undefined;
function tick() {
  clock.value = new Date().toTimeString().slice(0, 8);
}

/** The zone dot follows the connection, which is what gates half the screens. */
const zone = computed(() => (server.value?.reachable ? 'LINKED' : 'LOCAL'));

async function probe() {
  try {
    server.value = await desktop.serverStatus();
  } catch {
    server.value = {
      reachable: false,
      base_url: '',
      version: null,
      detail: 'Could not ask about the server.',
    };
  }
}

onMounted(() => {
  void probe();
  timer = window.setInterval(probe, 15_000);
  tick();
  ticking = window.setInterval(tick, 1000);
});
onUnmounted(() => {
  window.clearInterval(timer);
  window.clearInterval(ticking);
});
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

      <div class="server" :data-reachable="server?.reachable === true">
        <span class="dot" aria-hidden="true">●</span>
        <div class="server-text">
          <strong>{{ server?.reachable ? 'SERVER LINKED' : 'SERVER OFFLINE' }}</strong>
          <small>{{ server?.reachable ? server?.base_url : (server?.detail ?? 'Checking…') }}</small>
        </div>
      </div>
    </aside>

    <div class="main">
      <main class="content">
        <p v-if="server && !server.reachable" class="degraded">
          <span class="degraded__title">// SERVER UNREACHABLE //</span>
          The NAS is not answering, so publishing and anything else the server owns is
          unavailable. Local tools keep working normally.
        </p>
        <RouterView />
      </main>

      <footer class="statusbar">
        <span class="statusbar__zone" :data-reachable="server?.reachable === true">
          ● ZONE: {{ zone }}
        </span>
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

.server {
  display: flex;
  gap: var(--space-2);
  align-items: flex-start;
  padding: var(--space-3);
  border: var(--border-hair);
  border-radius: var(--radius-none);
  font-family: var(--font-body);
  font-size: 12px;
}
.server strong {
  font-family: var(--font-label);
  font-weight: 400;
  letter-spacing: 0.08em;
  color: var(--danger);
}
.server[data-reachable='true'] strong {
  color: var(--accent);
}
.dot {
  color: var(--danger);
  line-height: 1.2;
  flex: 0 0 auto;
}
.server[data-reachable='true'] .dot {
  color: var(--accent);
}
.server-text {
  display: grid;
  gap: 2px;
  min-width: 0;
}
.server-text small {
  color: var(--text-muted);
  overflow-wrap: anywhere;
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

.degraded {
  display: grid;
  gap: var(--space-2);
  border: 1px solid var(--accent-warm);
  color: var(--text);
  border-radius: var(--radius-none);
  padding: var(--space-3);
  margin-bottom: var(--space-5);
  font-size: 13px;
}
.degraded__title {
  font-family: var(--font-label);
  letter-spacing: 0.1em;
  color: var(--accent-warm);
  text-shadow: var(--glow-amber);
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
.statusbar__zone {
  color: var(--danger);
}
.statusbar__zone[data-reachable='true'] {
  color: var(--accent);
}
</style>
