<script setup lang="ts">
/**
 * The Rollchrome shell.
 *
 * Desktop-only (`docs/rollchrome-plan.md`, decision 2): single-photograph
 * non-destructive editing and batch grading, calling `rollchrome-core` and
 * `phototools-core` on this Mac. No server probe and no sync.
 */
import { onMounted, onUnmounted, ref } from 'vue';

const links = [
  { to: '/', label: 'Edit', step: '01' },
  { to: '/bulk-lut', label: 'Batch Grade', step: '02' },
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
        <span class="brand__name">ROLLCHROME</span>
        <span class="brand__sub">// EDIT</span>
      </div>

      <nav>
        <RouterLink v-for="link in links" :key="link.to" :to="link.to" class="nav-item">
          <span class="nav-item__step">{{ link.step }}</span>{{ link.label }}
        </RouterLink>
      </nav>

      <div class="spacer"></div>
    </aside>

    <div class="main">
      <main class="content">
        <RouterView />
      </main>

      <footer class="statusbar">
        <span class="statusbar__mid">ROLLCHROME v0.1.0</span>
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
