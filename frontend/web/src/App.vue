<script setup lang="ts">
import { onMounted } from 'vue';
import { authReady, initAuth, isConfigured, signIn, signOutOfPhotoTools, user } from './auth';

const links = [
  { to: '/', label: 'Home' },
  { to: '/library', label: 'Library' },
  { to: '/dates', label: 'Dates' },
  { to: '/rename', label: 'Rename' },
  { to: '/split', label: 'Split' },
  { to: '/contact-sheet', label: 'Sheet' },
  { to: '/transform', label: 'Transform' },
  { to: '/border', label: 'Border' },
  { to: '/tiff-to-jpeg', label: 'TIFF' },
];

onMounted(initAuth);
</script>

<template>
  <div class="shell">
    <header class="topbar">
      <RouterLink to="/" class="brand">PhotoTools</RouterLink>
      <div class="session">
        <template v-if="!authReady">…</template>
        <template v-else-if="!isConfigured">
          <span class="muted small">Sign-in unconfigured</span>
        </template>
        <template v-else-if="user">
          <span class="muted small">{{ user.email }}</span>
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
      <p v-if="authReady && !isConfigured" class="notice">
        This build has no Firebase configuration, so it cannot sign in. Set
        <code>VITE_FIREBASE_API_KEY</code>, <code>VITE_FIREBASE_AUTH_DOMAIN</code>,
        <code>VITE_FIREBASE_PROJECT_ID</code> and <code>VITE_FIREBASE_APP_ID</code>
        and rebuild. Requests will be refused until then.
      </p>
      <RouterView />
    </main>
  </div>
</template>

<style scoped>
.shell {
  min-height: 100dvh;
  display: grid;
  grid-template-rows: auto auto 1fr;
}
.topbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 12px 16px;
  border-bottom: 1px solid var(--rule);
  background: var(--surface);
}
.brand {
  font-weight: 650;
  letter-spacing: -0.01em;
  text-decoration: none;
  color: inherit;
  /* A tappable link needs a tappable height, not just its text box. */
  display: inline-flex;
  align-items: center;
  min-height: 44px;
}
.session { display: flex; align-items: center; gap: 10px; }
.small { font-size: 0.8rem; }

/* Horizontally scrollable tab strip: on a 390px phone the labels do not fit,
   and wrapping them would push the content below the fold. */
.tabs {
  display: flex;
  gap: 2px;
  overflow-x: auto;
  padding: 6px 10px;
  border-bottom: 1px solid var(--rule);
  background: var(--surface);
  scrollbar-width: thin;
}
.tab {
  flex: 0 0 auto;
  padding: 9px 12px;
  min-height: 40px;
  display: inline-flex;
  align-items: center;
  border-radius: 8px;
  text-decoration: none;
  color: var(--ink-soft);
  font-size: 0.9rem;
  white-space: nowrap;
}
.tab:hover { background: var(--surface-2); color: var(--ink); }
.tab.router-link-exact-active { background: var(--accent); color: var(--on-accent); }
.content { padding: 18px 16px 48px; }
.notice {
  border: 1px solid var(--warn);
  color: var(--warn);
  border-radius: 8px;
  padding: 10px 12px;
  margin-bottom: 18px;
  font-size: 0.88rem;
}
.notice code { font-family: var(--mono); font-size: 0.85em; }
</style>
