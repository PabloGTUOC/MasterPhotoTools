<script setup lang="ts">
/**
 * The Rollchrome shell.
 *
 * Desktop-only (`docs/rollchrome-plan.md`, decision 2): single-photograph
 * non-destructive editing and batch grading, calling `rollchrome-core` and
 * `phototools-core` on this Mac. No server probe and no sync.
 */
import { onMounted, onUnmounted, provide, reactive, ref, watch } from 'vue';
import { useRoute } from 'vue-router';
import { STATUS_STRIP, type StatusStripState } from './statusStrip';

const links = [
  { to: '/', label: 'Develop', abbr: 'DEV', step: '01' },
  { to: '/edit', label: 'Edit', abbr: 'EDT', step: '02' },
  { to: '/batch', label: 'Batch', abbr: 'BAT', step: '03' },
];

const statusStrip = reactive<StatusStripState>({
  hasDetails: false,
  detailsOpen: false,
  detailsInsetRight: 0,
});
provide(STATUS_STRIP, statusStrip);

const route = useRoute();
watch(
  () => route.path,
  () => {
    statusStrip.hasDetails = false;
    statusStrip.detailsOpen = false;
    statusStrip.detailsInsetRight = 0;
  },
);

function onKeyDown(e: KeyboardEvent) {
  if (e.key === 'Escape' && statusStrip.detailsOpen) {
    statusStrip.detailsOpen = false;
  }
}

/** The status bar's live clock (§5.8). */
const clock = ref('');
let ticking: number | undefined;
function tick() {
  clock.value = new Date().toTimeString().slice(0, 8);
}

onMounted(() => {
  tick();
  ticking = window.setInterval(tick, 1000);
  window.addEventListener('keydown', onKeyDown);
});
onUnmounted(() => {
  window.clearInterval(ticking);
  window.removeEventListener('keydown', onKeyDown);
});
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

      <div
        v-show="statusStrip.detailsOpen"
        id="status-strip-details"
        class="status-strip__details"
        data-testid="status-details"
        :style="{ right: `${statusStrip.detailsInsetRight}px` }"
      ></div>

      <footer
        class="statusbar status-strip"
        :class="{ 'status-strip--expanded': statusStrip.hasDetails }"
      >
        <div id="status-strip-line" class="status-strip__line"></div>
        <div class="status-strip__right">
          <button
            v-if="statusStrip.hasDetails"
            type="button"
            class="secondary status-details-toggle"
            data-testid="status-details-toggle"
            :aria-expanded="statusStrip.detailsOpen"
            @click="statusStrip.detailsOpen = !statusStrip.detailsOpen"
          >
            Details
          </button>
          <span class="statusbar__mid">ROLLCHROME v0.1.0</span>
          <span class="statusbar__right">{{ clock }}</span>
        </div>
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
  position: relative;
  display: grid;
  grid-template-rows: 1fr auto;
  height: 100vh;
  min-width: 0;
}

.content {
  min-height: 0;
  height: 100%;
  box-sizing: border-box;
  padding: var(--space-4) var(--space-5) var(--space-5);
  overflow-y: auto;
  display: flex;
  flex-direction: column;
}

/* --- status bar / status strip (§5.8) ------------------------------------- */

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
  box-sizing: border-box;
}

.status-strip--expanded {
  min-height: 40px;
}

.status-strip__line {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  display: flex;
  align-items: center;
}

.status-strip__right {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  flex-shrink: 0;
}

.status-details-toggle {
  min-height: 40px;
  height: 40px;
  padding: 0 var(--space-3);
  font-size: 12px;
  white-space: nowrap;
}

.status-strip__details {
  position: absolute;
  bottom: 40px;
  left: 0;
  right: 0;
  max-height: 40vh;
  overflow-y: auto;
  background: var(--bg-elevated);
  border-top: var(--border-hair);
  z-index: var(--z-status);
  box-sizing: border-box;
}
</style>

<style>
/* Rollchrome global rule: hide browser spin buttons on number inputs (UI-2) */
input[type="number"] {
  -moz-appearance: textfield;
  appearance: textfield;
}

input[type="number"]::-webkit-inner-spin-button,
input[type="number"]::-webkit-outer-spin-button {
  -webkit-appearance: none;
  display: none;
  margin: 0;
}
</style>
