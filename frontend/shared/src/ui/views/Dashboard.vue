<script setup lang="ts">
import { onMounted, ref } from 'vue';
import { api } from '@host/api';
import { sharedToolLinks } from '@ui/routes';

const version = ref<string | null>(null);
const reachable = ref<boolean | null>(null);

/**
 * The longer name and the one-line note, by path.
 *
 * Only this lives here. The cards themselves are built from `sharedToolLinks`,
 * so the dashboard cannot disagree with the tab bar about what exists or what
 * order it comes in — which it had: this list was hand-maintained, and by the
 * time anybody looked it was missing Geotag and RAW to JPEG entirely.
 */
const DESCRIBED: Record<string, { name: string; note: string }> = {
  '/dates': { name: 'Dates', note: 'Scan and repair capture dates' },
  '/rename': { name: 'Rename', note: 'Batch rename to a sortable scheme' },
  '/geotag': { name: 'Geotag', note: 'Place photographs from a GPS track' },
  '/tiff-to-jpeg': { name: 'TIFF to JPEG', note: 'Convert scanner output' },
  '/raw-to-jpeg': { name: 'RAW to JPEG', note: 'Develop camera raw files' },
  '/split': { name: 'Half-frame split', note: 'Separate two-up film scans' },
  '/border': { name: 'Print border', note: 'Place on a print canvas' },
  '/contact-sheet': { name: 'Contact sheet', note: 'Proof sheet from a folder' },
  '/transform': { name: 'Transform', note: 'Rotate, resize, convert' },
};

const tools = sharedToolLinks.map((link) => ({
  to: link.to,
  step: link.step,
  name: DESCRIBED[link.to]?.name ?? link.label,
  note: DESCRIBED[link.to]?.note ?? '',
}));

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
    <header class="hero">
      <span class="hero__kicker">// ARCHIVE TERMINAL //</span>
      <h1 class="hero__title">PhotoTools</h1>
      <p class="muted">Archive tools for the library on the NAS.</p>
    </header>

    <p class="status" :data-ok="reachable === true">
      <template v-if="reachable === null">Checking the server<span class="cursor">_</span></template>
      <template v-else-if="reachable">Server reachable — version {{ version }}</template>
      <template v-else>Server unreachable. Archive tools will not work.</template>
    </p>

    <nav class="grid">
      <RouterLink v-for="tool in tools" :key="tool.to" :to="tool.to" class="card">
        <span class="card-step">{{ tool.step }}</span>
        <span class="card-name">{{ tool.name }}</span>
        <span class="card-note">{{ tool.note }}</span>
      </RouterLink>
    </nav>
  </section>
</template>

<style scoped>
.dash {
  display: grid;
  gap: var(--space-6);
}

.hero {
  display: grid;
  gap: var(--space-2);
}
.hero__kicker {
  font-family: var(--font-label);
  font-size: 12px;
  letter-spacing: 0.12em;
  text-transform: uppercase;
  color: var(--accent);
}
/* display-lg, down-scaled on a phone (§7). Glow is permitted here — it is a
   heading at display size, not body copy (§2.3). */
.hero__title {
  font-size: 48px;
  color: var(--text-heading);
  text-shadow: var(--glow-phosphor);
}
@media (min-width: 768px) {
  .hero__title {
    font-size: 72px;
  }
}

.status {
  font-family: var(--font-label);
  font-size: 13px;
  letter-spacing: 0.08em;
  text-transform: uppercase;
  color: var(--danger);
}
.status[data-ok='true'] {
  color: var(--accent);
}

.grid {
  display: grid;
  gap: var(--space-3);
  grid-template-columns: 1fr;
}
@media (min-width: 34rem) {
  .grid {
    grid-template-columns: repeat(auto-fill, minmax(15rem, 1fr));
  }
}

.card {
  display: grid;
  gap: var(--space-1);
  padding: var(--space-4);
  border: var(--border-hair);
  border-radius: var(--radius-none);
  background: var(--bg-elevated);
  text-decoration: none;
  color: inherit;
  /* Comfortable one-handed target on a phone. */
  min-height: 3.5rem;
  transition: border-color var(--dur-fast) var(--ease);
}
.card:hover,
.card:focus-visible {
  border-color: var(--accent);
  box-shadow: var(--glow-phosphor);
}

/* The step, above the name: small type, so no glow. */
.card-step {
  font-family: var(--font-label);
  font-size: 12px;
  letter-spacing: 0.1em;
  color: var(--text-disabled);
}
.card-name {
  font-family: var(--font-label);
  font-size: 15px;
  letter-spacing: 0.08em;
  text-transform: uppercase;
  color: var(--text-heading);
}
.card-note {
  color: var(--text-muted);
  font-size: 13px;
}
</style>
