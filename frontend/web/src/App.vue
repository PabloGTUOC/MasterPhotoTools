<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue';
import { authReady, initAuth, isConfigured, signIn, signOutOfPhotoTools, user } from './auth';

const links = [
  { to: '/', label: 'Home' },
  { to: '/publish', label: 'Publish' },
  { to: '/dates', label: 'Dates' },
  { to: '/rename', label: 'Rename' },
  { to: '/split', label: 'Split' },
  { to: '/contact-sheet', label: 'Sheet' },
  { to: '/transform', label: 'Transform' },
  { to: '/border', label: 'Border' },
  { to: '/tiff-to-jpeg', label: 'TIFF' },
];

/** The status bar's live clock (§5.8), monospaced and always two digits. */
const clock = ref('');
let ticking: number | undefined;

function tick() {
  clock.value = new Date().toTimeString().slice(0, 8);
}

/** `USER: PABLO` — the local part, uppercased, or the signed-out state. */
const who = computed(() => {
  if (!authReady.value) return '—';
  if (!isConfigured.value) return 'UNCONFIGURED';
  return user.value?.email?.split('@')[0]?.toUpperCase() ?? 'ANONYMOUS';
});

onMounted(() => {
  initAuth();
  tick();
  ticking = window.setInterval(tick, 1000);
});
onUnmounted(() => window.clearInterval(ticking));
</script>

<template>
  <div class="shell crt-boot">
    <header class="topbar">
      <RouterLink to="/" class="brand">
        <span class="brand__name">PHOTOTOOLS</span>
        <span class="brand__sub">// ARCHIVE</span>
      </RouterLink>

      <div class="session">
        <template v-if="!authReady"><span class="cursor">_</span></template>
        <template v-else-if="!isConfigured">
          <span class="badge badge--warn">NO AUTH</span>
        </template>
        <template v-else-if="user">
          <span class="session__who">{{ user.email }}</span>
          <button type="button" class="ghost" @click="signOutOfPhotoTools">Sign out</button>
        </template>
        <button v-else type="button" class="primary" @click="signIn">Sign in</button>
      </div>
    </header>

    <nav class="tabs" aria-label="Tools">
      <RouterLink v-for="link in links" :key="link.to" :to="link.to" class="tab">
        {{ link.label }}
      </RouterLink>
    </nav>

    <main class="content">
      <div v-if="authReady && !isConfigured" class="notice">
        <span class="notice__title">// NO SIGN-IN CONFIGURED //</span>
        <p>This build has no Firebase configuration, so it cannot sign in. Requests are refused
          until these four are set and it is rebuilt:</p>
        <ul class="notice__vars">
          <li>VITE_FIREBASE_API_KEY</li>
          <li>VITE_FIREBASE_AUTH_DOMAIN</li>
          <li>VITE_FIREBASE_PROJECT_ID</li>
          <li>VITE_FIREBASE_APP_ID</li>
        </ul>
      </div>
      <RouterView />
    </main>

    <footer class="statusbar">
      <span class="statusbar__zone">● ZONE: ARCHIVE</span>
      <span class="statusbar__mid">PHOTOTOOLS v0.1.0</span>
      <span class="statusbar__right">USER: {{ who }} // {{ clock }}</span>
    </footer>
  </div>
</template>

<style scoped>
.shell {
  min-height: 100dvh;
  display: grid;
  /* The status bar is the last row rather than fixed: a fixed bar overlaps the
     final control on a short phone viewport, which check:layout would pass and
     a thumb would not. */
  grid-template-rows: auto auto 1fr auto;
}

/* --- top bar (§5.1) ------------------------------------------------------ */

.topbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-3);
  padding: 0 var(--space-4);
  min-height: var(--nav-h);
  border-bottom: var(--border-hair);
  background: var(--bg-elevated);
}

.brand {
  display: inline-flex;
  align-items: baseline;
  gap: var(--space-2);
  min-height: 44px;
  text-decoration: none;
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

.session {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}
.session__who {
  font-family: var(--font-body);
  font-size: 12px;
  color: var(--text-muted);
  /* An address is long and the bar is 48px; truncate rather than wrap it. */
  max-width: 16ch;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* --- tabs (§5.1): terminal mode selectors -------------------------------- */

.tabs {
  display: flex;
  gap: var(--space-4);
  /* On a 390px phone the labels do not fit, and wrapping them pushes content
     below the fold. A scroller keeps the terminal row intact (§7). */
  overflow-x: auto;
  padding: 0 var(--space-4);
  border-bottom: var(--border-hair);
  background: var(--bg-elevated);
  scrollbar-width: none;
}
.tabs::-webkit-scrollbar {
  display: none;
}

.tab {
  flex: 0 0 auto;
  display: inline-flex;
  align-items: center;
  min-height: 44px;
  padding: 0 var(--space-1);
  font-family: var(--font-label);
  font-size: 13px;
  letter-spacing: 0.1em;
  text-transform: uppercase;
  text-decoration: none;
  white-space: nowrap;
  color: var(--text-disabled);
  border-bottom: 2px solid transparent;
  transition: color var(--dur-fast) var(--ease);
}
.tab:hover {
  color: var(--text);
}
.tab.router-link-exact-active {
  color: var(--accent);
  border-bottom-color: var(--accent);
  text-shadow: var(--glow-phosphor);
}

.content {
  padding: var(--space-5) var(--space-4) var(--space-7);
}

.notice {
  display: grid;
  gap: var(--space-2);
  border: 1px solid var(--accent-warm);
  color: var(--text);
  padding: var(--space-3);
  margin-bottom: var(--space-5);
  font-size: 13px;
}
.notice__title {
  font-family: var(--font-label);
  letter-spacing: 0.1em;
  color: var(--accent-warm);
  text-shadow: var(--glow-amber);
}
.notice__vars {
  list-style: none;
  display: grid;
  gap: var(--space-1);
  font-family: var(--font-body);
  color: var(--accent-warm);
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
  /* One line, always. Wrapping doubles the bar's height and the layout below
     it was measured against 28px. */
  white-space: nowrap;
}
.statusbar__zone {
  color: var(--accent-warm);
}
.statusbar__right {
  overflow: hidden;
  text-overflow: ellipsis;
}
/* Tracking is what overflows first on a narrow phone; the values matter more
   than the letter-spacing does. */
@media (max-width: 420px) {
  .statusbar {
    letter-spacing: 0.04em;
    font-size: 11px;
    gap: var(--space-2);
  }
}
/* The centre label is the first thing to go on a phone (§5.8 mobile). */
.statusbar__mid {
  display: none;
}
@media (min-width: 768px) {
  .statusbar__mid {
    display: inline;
  }
}
</style>
