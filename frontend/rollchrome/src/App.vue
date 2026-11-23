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
  { to: '/', label: 'Develop', abbr: 'DEV', step: '01' },
  { to: '/edit', label: 'Edit', abbr: 'EDT', step: '02' },
  { to: '/batch', label: 'Batch', abbr: 'BAT', step: '03' },
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
      <div class="brand" title="Rollchrome">
        <span class="brand__name">RC</span>
      </div>

      <nav>
        <RouterLink
          v-for="link in links"
          :key="link.to"
          :to="link.to"
          class="nav-item"
          :aria-label="link.label"
          :title="link.label"
        >
          <span class="nav-item__step">{{ link.step }}</span>
          <span class="nav-item__label">{{ link.abbr }}</span>
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
  grid-template-columns: 56px minmax(0, 1fr);
  height: 100vh;
  overflow: hidden;
}

/* --- sidebar (§6.4): the roll list pattern, applied to tools ------------- */

.sidebar {
  display: flex;
  flex-direction: column;
  width: 56px;
  box-sizing: border-box;
  padding: var(--space-2) 0;
  border-right: var(--border-hair);
  background: var(--bg-elevated);
}

.brand {
  display: flex;
  align-items: center;
  justify-content: center;
  padding: var(--space-2) 0 var(--space-4);
  user-select: none;
}
.brand__name {
  font-family: var(--font-ui);
  font-weight: 500;
  font-size: 14px;
  letter-spacing: 0.08em;
  color: var(--text-heading);
}

nav {
  display: flex;
  flex-direction: column;
  gap: 1px;
}

.nav-item {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  width: 100%;
  min-height: 56px;
  box-sizing: border-box;
  padding: var(--space-1) 0;
  text-decoration: none;
  color: var(--text-disabled);
  /* The active marker is a left border, so the tab never moves as it
     activates — a shifting nav is the fastest way to feel cheap. */
  border-left: 2px solid transparent;
  transition: color var(--dur-fast) var(--ease), background var(--dur-fast) var(--ease);
}
.nav-item:hover {
  color: var(--text);
  background: var(--bg-panel);
}

.nav-item__step {
  font-family: var(--font-display);
  font-size: 20px;
  line-height: 1.1;
  color: inherit;
  text-shadow: none;
}

.nav-item__label {
  font-family: var(--font-label);
  font-size: 11px;
  line-height: 1.1;
  letter-spacing: 0.1em;
  text-transform: uppercase;
  color: inherit;
  text-shadow: none;
}

.nav-item.router-link-exact-active {
  color: var(--accent);
  border-left-color: var(--accent);
  background: var(--bg-panel);
}
.nav-item.router-link-exact-active .nav-item__step {
  color: var(--accent);
  text-shadow: var(--glow-phosphor);
}
.nav-item.router-link-exact-active .nav-item__label {
  color: var(--accent);
  text-shadow: none;
}

.spacer {
  flex: 1;
}

/* --- main column --------------------------------------------------------- */

.main {
  display: grid;
  grid-template-rows: 1fr auto;
  height: 100vh;
  min-width: 0;
}

.content {
  min-height: 0;
  padding: var(--space-4) var(--space-5) var(--space-5);
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
