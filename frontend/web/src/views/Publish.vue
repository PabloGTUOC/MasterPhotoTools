<script setup lang="ts">
/**
 * Publishing a session to Google Photos (Phase 13, task 4).
 *
 * A web view, and deliberately not a shared one. The Google refresh token lives
 * on exactly one machine (§2.3), so publishing is something only a build
 * talking to the server can do.
 *
 * **Publish is unreachable until a dry run has been reviewed.** That is §9.2
 * rule 3, and it is enforced on the server as well — the button is disabled
 * here because a disabled button is a better explanation than a rejection, not
 * because the server trusts this screen.
 */
import { computed, onMounted, ref } from 'vue';
import type { ConnectorStatus, PublishPlan } from '@phototools/shared';
import JobProgress from '@ui/components/JobProgress.vue';
import { server } from '../api';

const sessionId = ref('');
const connector = ref<ConnectorStatus | null>(null);
const plan = ref<PublishPlan | null>(null);
const jobId = ref<string | null>(null);
const busy = ref(false);
const failure = ref<string | null>(null);

/**
 * The plan the person actually looked at.
 *
 * Reset whenever the session changes, because a dry run of one card says
 * nothing about another — and the whole safeguard is that somebody reviewed
 * *this* one.
 */
const reviewed = ref(false);

const canPublish = computed(
  () =>
    reviewed.value &&
    !busy.value &&
    (plan.value?.items.length ?? 0) > 0 &&
    connector.value?.connected === true,
);

async function guard<T>(work: () => Promise<T>): Promise<T | undefined> {
  busy.value = true;
  failure.value = null;
  try {
    return await work();
  } catch (e) {
    failure.value = e instanceof Error ? e.message : String(e);
    return undefined;
  } finally {
    busy.value = false;
  }
}

async function refreshConnector() {
  const status = await guard(() => server.googleStatus());
  connector.value = status ?? {
    // A check that could not be made is not a connection that is fine. Leaving
    // this null would sit on "Checking the connection…" for ever.
    connected: false,
    scope: null,
    connected_at: null,
    needs_reauthorisation: false,
    detail: 'Could not ask the server about Google Photos.',
  };
}

async function connect() {
  const url = await guard(() => server.googleConnect());
  if (url) window.location.href = url;
}

async function disconnect() {
  await guard(() => server.googleDisconnect());
  await refreshConnector();
}

async function dryRun() {
  const id = sessionId.value.trim();
  if (!id) {
    failure.value = 'Enter the session the desktop reported when it handed the card over.';
    return;
  }
  const result = await guard(() => server.publishDryRun(id));
  if (result) {
    plan.value = result;
    reviewed.value = true;
  }
}

async function publish() {
  const id = await guard(() => server.publish(sessionId.value.trim()));
  if (id) {
    jobId.value = id;
    // One dry run authorises one publish. Anything after this needs another
    // look, because what is on the server has changed.
    reviewed.value = false;
  }
}

function onSessionChanged() {
  plan.value = null;
  reviewed.value = false;
}

function megabytes(bytes: number): string {
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

onMounted(refreshConnector);
</script>

<template>
  <section class="page">
    <header class="head">
      <h1>Publish</h1>
      <p class="muted">
        Send a handed-over session to Google Photos. The API cannot delete what
        it has created, so a dry run comes first — always.
      </p>
    </header>

    <section class="connector" aria-label="Google Photos connection">
      <template v-if="connector">
        <span class="pill" :data-tone="connector.connected ? 'ok' : 'bad'">
          {{ connector.connected ? 'connected' : 'not connected' }}
        </span>
        <span v-if="connector.detail" class="muted small">{{ connector.detail }}</span>
        <button
          v-if="connector.connected"
          type="button"
          class="ghost"
          :disabled="busy"
          @click="disconnect"
        >
          Disconnect
        </button>
        <button v-else type="button" class="secondary" :disabled="busy" @click="connect">
          {{ connector.needs_reauthorisation ? 'Reconnect' : 'Connect Google Photos' }}
        </button>
      </template>
      <span v-else class="muted small">Checking the connection…</span>
    </section>

    <label class="field">
      <span>Session</span>
      <input
        v-model="sessionId"
        type="text"
        placeholder="the id the desktop reported after handing over"
        @input="onSessionChanged"
      />
    </label>

    <div class="row">
      <button type="button" class="secondary" :disabled="busy" @click="dryRun">
        Dry run
      </button>
      <button type="button" class="primary" :disabled="!canPublish" @click="publish">
        Publish
      </button>
    </div>

    <p v-if="!reviewed" class="muted small" data-testid="gate-explanation">
      Publish is unavailable until a dry run for this session has been reviewed.
    </p>

    <p v-if="failure" class="error">{{ failure }}</p>

    <section v-if="plan" class="plan" aria-live="polite">
      <h2>{{ plan.items.length }} photograph{{ plan.items.length === 1 ? '' : 's' }}</h2>
      <ul class="facts">
        <li>{{ megabytes(plan.total_bytes) }} to upload</li>
        <li>{{ plan.upload_requests }} upload request{{ plan.upload_requests === 1 ? '' : 's' }}</li>
        <li>
          {{ plan.batch_create_requests }} batch call{{ plan.batch_create_requests === 1 ? '' : 's' }}
          <span class="muted">(fifty photographs each, at most)</span>
        </li>
      </ul>

      <p v-if="plan.resuming.created" class="muted small">
        {{ plan.resuming.created }} already published and will be left alone.
      </p>

      <p v-if="plan.resuming.unconfirmed" class="warning">
        {{ plan.resuming.unconfirmed }} shot{{ plan.resuming.unconfirmed === 1 ? '' : 's' }}
        were sent to Google without an answer coming back. They are not retried,
        because a second attempt would duplicate any that succeeded and Google
        Photos cannot delete. Look for them in the library before publishing
        again.
      </p>

      <details v-if="plan.skipped.length">
        <summary>{{ plan.skipped.length }} skipped</summary>
        <ul class="skipped">
          <li v-for="skip in plan.skipped" :key="skip.stem" class="small">
            <strong>{{ skip.stem }}</strong> — {{ skip.reason }}
          </li>
        </ul>
      </details>

      <details v-if="plan.items.length">
        <summary>What would be published</summary>
        <ul class="items">
          <li v-for="item in plan.items" :key="item.shot_id" class="small mono">
            {{ item.stem }}
          </li>
        </ul>
      </details>
    </section>

    <JobProgress :job-id="jobId" />
  </section>
</template>

<style scoped>
.page { display: grid; gap: 16px; padding: 16px; max-width: 780px; }
.head h1 {
  font-size: 40px;
}
.row { display: flex; gap: 10px; flex-wrap: wrap; }
.connector {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
  padding: 10px 12px;
  border: 1px solid var(--border);
  border-radius: var(--radius-none);
  background: var(--bg-panel);
}
.pill {
  font-size: 13px;
  padding: 4px 10px;
  border-radius: var(--radius-none);
  border: 1px solid var(--border);
  color: var(--text-muted);
}
.pill[data-tone='ok'] { color: var(--accent); border-color: var(--accent); }
.pill[data-tone='bad'] { color: var(--danger); border-color: var(--danger); }
.plan {
  display: grid;
  gap: 10px;
  padding: 14px;
  border: 1px solid var(--border);
  border-radius: var(--radius-none);
  background: var(--bg-panel);
}
.plan h2 {
  font-family: var(--font-label);
  font-size: 18px;
  letter-spacing: 0.1em;
}
.facts { list-style: none; display: grid; gap: 4px; font-size: 14px; }
.skipped, .items { list-style: none; display: grid; gap: 3px; padding-top: 8px; max-height: 40vh; overflow-y: auto; }
.warning {
  padding: 10px 12px;
  border: 1px solid var(--accent-warm);
  border-radius: var(--radius-none);
  color: var(--accent-warm);
  font-size: 13px;
}
.small { font-size: 13px; }
.mono { font-family: var(--font-body); }
summary { cursor: pointer; font-size: 14px; min-height: 32px; }
</style>
