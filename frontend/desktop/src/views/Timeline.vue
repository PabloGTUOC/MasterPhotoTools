<script setup lang="ts">
/**
 * The Timeline tab on the Mac: the shared screen, with the server above it.
 *
 * **A wrapper rather than a prop on the shared view.** Syncing is the desktop's
 * alone — the server cannot sync with itself — and `views/` may only use what
 * both transports implement (`frontend/shared/src/ui/README.md`). Wrapping
 * keeps the shared screen identical in both applications and puts the
 * machine-specific part where its reason is visible.
 *
 * The server's address lives here too, beside the button that needs it. It had
 * no screen at all until now: the address sat in `server.json` and the token in
 * the Keychain, both settable only by hand, which is a fine way to own a
 * feature nobody can turn on.
 */
import { onMounted, ref } from 'vue';
import SharedTimeline from '@ui/views/Timeline.vue';
import { getServerSettings, setServerSettings } from '../api';
import { lastFailed, lastSync, runSync, syncing } from '../sync';

const address = ref('');
const token = ref('');
const saving = ref(false);
const saved = ref<string | null>(null);
const failure = ref<string | null>(null);

onMounted(async () => {
  try {
    const settings = await getServerSettings();
    address.value = settings.base_url;
    token.value = settings.auth_token ?? '';
  } catch (e) {
    failure.value = e instanceof Error ? e.message : String(e);
  }
});

async function save() {
  saving.value = true;
  failure.value = null;
  try {
    await setServerSettings({
      base_url: address.value.trim().replace(/\/+$/, ''),
      // Empty clears it: a token removed from the field has to leave the
      // Keychain, or the next launch restores it.
      auth_token: token.value.trim() || null,
    });
    saved.value = 'Saved.';
  } catch (e) {
    saved.value = null;
    failure.value = e instanceof Error ? e.message : String(e);
  } finally {
    saving.value = false;
  }
}
</script>

<template>
  <section class="server" aria-label="The server this Mac syncs with">
    <header>
      <h2>Server</h2>
      <p class="muted">
        The timeline is kept on both machines and each works when the other is off. Syncing carries
        <strong>geopositions only</strong> — tracks, their fixes and deletions. Nothing about cards,
        photographs or publishing crosses between them.
      </p>
    </header>

    <div class="row">
      <label class="field">
        <span>Address</span>
        <input v-model="address" type="text" placeholder="http://192.168.50.174:2343" />
        <small class="muted">Where the server answers. On the local network it is faster.</small>
      </label>

      <label class="field">
        <span>Token</span>
        <input v-model="token" type="password" placeholder="the server's admin token" />
        <small class="muted">Kept in the macOS Keychain, never in a file.</small>
      </label>
    </div>

    <div class="row">
      <button type="button" class="secondary" :disabled="saving" @click="save">Save</button>
      <button type="button" class="primary" :disabled="syncing" @click="runSync(false)">
        {{ syncing ? 'Syncing…' : 'Sync now' }}
      </button>
      <span v-if="saved" class="muted small">{{ saved }}</span>
    </div>

    <p v-if="lastSync" class="note" :data-failed="lastFailed" role="status">{{ lastSync }}</p>
    <p v-if="failure" class="error" role="alert">{{ failure }}</p>
  </section>

  <SharedTimeline />
</template>

<style scoped>
.server {
  display: grid;
  gap: var(--space-3);
  border: 1px solid var(--border);
  padding: var(--space-4);
  margin-bottom: var(--space-5);
}

.server h2 {
  font-family: var(--font-label);
  font-size: 13px;
  letter-spacing: 0.1em;
  text-transform: uppercase;
  color: var(--text-heading);
}

.row {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-3);
  align-items: flex-end;
}
.row > .field {
  flex: 1 1 260px;
}

.note[data-failed='true'] {
  color: var(--accent-warm);
}

.small {
  font-size: 12px;
}
</style>
