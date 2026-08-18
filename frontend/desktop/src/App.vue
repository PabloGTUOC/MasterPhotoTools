<script setup lang="ts">
/**
 * The desktop shell.
 *
 * Task 6 — graceful degradation: the server's reachability is shown, and
 * server-backed features disable with a clear indicator. Nothing local breaks
 * when the NAS is off.
 */
import { onMounted, onUnmounted, ref } from 'vue';
import { desktop, type ServerStatus } from './api';

const links = [
  { to: '/', label: 'Library' },
  { to: '/ingest', label: 'Ingest' },
  { to: '/dates', label: 'Dates' },
  { to: '/rename', label: 'Rename' },
  { to: '/split', label: 'Split' },
  { to: '/contact-sheet', label: 'Sheet' },
  { to: '/transform', label: 'Transform' },
  { to: '/border', label: 'Border' },
  { to: '/tiff-to-jpeg', label: 'TIFF' },
];

const server = ref<ServerStatus | null>(null);
let timer: number | undefined;

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
});
onUnmounted(() => window.clearInterval(timer));
</script>

<template>
  <div class="shell">
    <aside class="sidebar">
      <div class="brand">PhotoTools</div>
      <nav>
        <RouterLink v-for="link in links" :key="link.to" :to="link.to" class="nav-item">
          {{ link.label }}
        </RouterLink>
      </nav>
      <div class="spacer"></div>

      <div class="server" :data-reachable="server?.reachable === true">
        <span class="dot" aria-hidden="true"></span>
        <div class="server-text">
          <strong>{{ server?.reachable ? 'Server connected' : 'Server offline' }}</strong>
          <small>{{ server?.reachable ? server?.base_url : (server?.detail ?? 'Checking…') }}</small>
        </div>
      </div>
    </aside>

    <main class="content">
      <p v-if="server && !server.reachable" class="degraded">
        The NAS is not answering, so publishing and anything else the server owns
        is unavailable. Local tools below keep working normally.
      </p>
      <RouterView />
    </main>
  </div>
</template>

<style scoped>
.shell { display: grid; grid-template-columns: 232px 1fr; min-height: 100vh; }
.sidebar {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 16px 12px;
  border-right: 1px solid var(--rule);
  background: var(--surface-2);
}
.brand { font-weight: 650; padding: 6px 10px 14px; }
nav { display: grid; gap: 2px; }
.nav-item {
  padding: 9px 12px;
  border-radius: 8px;
  text-decoration: none;
  color: var(--ink-soft);
  font-size: 0.92rem;
}
.nav-item:hover { background: var(--surface); color: var(--ink); }
.nav-item.router-link-exact-active { background: var(--accent); color: var(--on-accent); }
.spacer { flex: 1; }
.server {
  display: flex;
  gap: 9px;
  align-items: flex-start;
  padding: 10px;
  border: 1px solid var(--rule);
  border-radius: 8px;
  font-size: 0.8rem;
}
.dot {
  width: 8px; height: 8px; border-radius: 50%;
  background: var(--critical);
  margin-top: 5px;
  flex: 0 0 auto;
}
.server[data-reachable='true'] .dot { background: var(--ok); }
.server-text { display: grid; gap: 2px; min-width: 0; }
.server-text small { color: var(--ink-faint); word-break: break-all; }
.content { padding: 24px 28px 48px; overflow-y: auto; }
.degraded {
  border: 1px solid var(--warn);
  color: var(--warn);
  border-radius: 8px;
  padding: 10px 12px;
  margin-bottom: 20px;
  font-size: 0.88rem;
}
</style>
