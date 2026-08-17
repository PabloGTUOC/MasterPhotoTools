<script setup lang="ts">
import { onMounted, ref } from 'vue';
import { api } from '@host/api';

const version = ref<string | null>(null);
const reachable = ref<boolean | null>(null);

const tools = [
  { to: '/library', name: 'Library', note: 'Browse the archive' },
  { to: '/dates', name: 'Dates', note: 'Scan and repair capture dates' },
  { to: '/rename', name: 'Rename', note: 'Batch rename to a sortable scheme' },
  { to: '/split', name: 'Half-frame split', note: 'Separate two-up film scans' },
  { to: '/contact-sheet', name: 'Contact sheet', note: 'Proof sheet from a folder' },
  { to: '/transform', name: 'Transform', note: 'Rotate, resize, convert' },
  { to: '/border', name: 'Print border', note: 'Place on a print canvas' },
  { to: '/tiff-to-jpeg', name: 'TIFF to JPEG', note: 'Convert scanner output' },
];

onMounted(async () => {
  try {
    const health = await api.health();
    version.value = health.version;
    reachable.value = true;
  } catch {
    reachable.value = false;
  }
});
</script>

<template>
  <section class="dash">
    <header>
      <h1>PhotoTools</h1>
      <p class="muted">Archive tools for the library on the NAS.</p>
    </header>

    <p class="status" :data-ok="reachable === true">
      <template v-if="reachable === null">Checking the server…</template>
      <template v-else-if="reachable">Server reachable — version {{ version }}</template>
      <template v-else>Server unreachable. Archive tools will not work.</template>
    </p>

    <nav class="grid">
      <RouterLink v-for="tool in tools" :key="tool.to" :to="tool.to" class="card">
        <span class="card-name">{{ tool.name }}</span>
        <span class="card-note">{{ tool.note }}</span>
      </RouterLink>
    </nav>
  </section>
</template>

<style scoped>
.dash { display: grid; gap: 20px; }
h1 { font-size: 1.6rem; letter-spacing: -0.02em; }
.status {
  font-family: var(--mono);
  font-size: 0.85rem;
  color: var(--critical);
}
.status[data-ok='true'] { color: var(--ok); }
.grid {
  display: grid;
  gap: 10px;
  grid-template-columns: 1fr;
}
@media (min-width: 34rem) {
  .grid { grid-template-columns: repeat(auto-fill, minmax(15rem, 1fr)); }
}
.card {
  display: grid;
  gap: 4px;
  padding: 16px;
  border: 1px solid var(--rule);
  border-radius: 10px;
  background: var(--surface-2);
  text-decoration: none;
  color: inherit;
  /* Comfortable one-handed target on a phone. */
  min-height: 3.5rem;
}
.card:hover, .card:focus-visible { border-color: var(--accent); }
.card-name { font-weight: 600; }
.card-note { color: var(--ink-soft); font-size: 0.88rem; }
</style>
