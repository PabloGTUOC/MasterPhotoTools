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
import type { ConnectorStatus, FolderPublishPlan, PublishPlan } from '@phototools/shared';
import JobProgress from '@ui/components/JobProgress.vue';
import PathListField from '@ui/components/PathListField.vue';
import { useRoots } from '@ui/useRoots';
import { api } from '../api';
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

// --- publishing a folder (docs/publish-folder-plan.md) --------------------
//
// The road that lets the tools run before anything is published. The session
// road below it still works and is still the one the specification describes;
// both are here while this one is being verified against real photographs
// (MV-16), and the older one is retired in a change of its own afterwards.

const { roots } = useRoots();
const list = (path: string) => api.list(path);

/** What is sitting in the publishing folder right now. */
const folder = ref<FolderPublishPlan | null>(null);
/** Folders or files to copy in before publishing. */
const sources = ref('');
const folderReviewed = ref(false);
const filled = ref<string | null>(null);

/**
 * The dry run is bound to the exact bytes in the folder.
 *
 * The session id is derived from every file's path and content hash, so adding
 * a file — or geotagging one, which rewrites it — produces a different id and
 * the review no longer counts. This holds the id that was reviewed so the
 * screen can notice.
 */
const reviewedSession = ref<string | null>(null);

const folderChanged = computed(
  () => reviewedSession.value !== null && reviewedSession.value !== folder.value?.session_id,
);

const canPublishFolder = computed(
  () =>
    folderReviewed.value &&
    !folderChanged.value &&
    !busy.value &&
    (folder.value?.items.length ?? 0) > 0 &&
    connector.value?.connected === true,
);

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

async function readFolder() {
  const contents = await guard(() => server.publishingFolder());
  if (contents) folder.value = contents;
}

async function fill() {
  const paths = sources.value
    .split('\n')
    .map((p) => p.trim())
    .filter(Boolean);
  if (!paths.length) {
    failure.value = 'Choose a folder or files to copy in.';
    return;
  }

  const result = await guard(() => api.fillPublishing(paths));
  if (result) {
    filled.value = result.summary;
    // What is in the folder has changed, so any earlier review is void.
    folderReviewed.value = false;
    await readFolder();
  }
}

async function folderDryRun() {
  const result = await guard(() => server.planFolderPublish());
  if (result) {
    folder.value = result;
    folderReviewed.value = true;
    reviewedSession.value = result.session_id;
  }
}

async function publishTheFolder() {
  const id = await guard(() => server.publishFolder());
  if (id) {
    jobId.value = id;
    // One review authorises one publish, and the folder is emptied by a
    // successful one — so whatever is there next is something else.
    folderReviewed.value = false;
    reviewedSession.value = null;
    await readFolder();
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

onMounted(async () => {
  await refreshConnector();
  // The folder is read on opening the tab: whatever is in it is what would be
  // published, however it got there.
  await readFolder();
});
</script>

<template>
  <section class="page">
    <header class="head">
      <h1>Publish</h1>
      <p class="muted">
        Send photographs to Google Photos. The API cannot delete what it has
        created, so a dry run comes first — always.
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

    <!-- The folder road. Everything in the publishing folder is uploaded and
         then removed from it; the tools run on the way in. -->
    <section class="road" aria-label="Publish a folder">
      <h2 class="road__head">// THE PUBLISHING FOLDER //</h2>

      <p v-if="!folder" class="muted small">Reading the folder…</p>

      <template v-else>
        <p class="muted small">
          Everything here is uploaded and then <strong>removed from this folder</strong>. It is
          never the only copy — the tools cannot write here, so what is in it was copied in. Files
          you added from Finder are published too.
        </p>

        <ul class="facts">
          <li>
            <strong>{{ folder.items.length }}</strong>
            photograph{{ folder.items.length === 1 ? '' : 's' }} to upload
          </li>
          <li>{{ megabytes(folder.total_bytes) }}</li>
          <li v-if="folder.skipped.length">
            <strong>{{ folder.skipped.length }}</strong> will stay
          </li>
        </ul>

        <div v-if="folder.items.length" class="rows">
          <div v-for="item in folder.items" :key="item.shot_id" class="rows__row">
            <span class="rows__name">{{ item.file_name }}</span>
            <span class="rows__size">{{ megabytes(item.bytes) }}</span>
          </div>
        </div>

        <!-- A file that will not be uploaded is a file that will still be here
             afterwards, and somebody should know which and why. -->
        <ul v-if="folder.skipped.length" class="skipped">
          <li v-for="skip in folder.skipped" :key="skip.stem">
            <span aria-hidden="true">·</span> {{ skip.stem }} — {{ skip.reason }}
          </li>
        </ul>

        <PathListField
          v-model="sources"
          label="Copy folders or files in first (optional)"
          placeholder="/library/2026/berlin"
          :roots="roots"
          :list="list"
        />

        <p v-if="filled" class="note" role="status">{{ filled }}</p>

        <div class="row">
          <button type="button" class="ghost" :disabled="busy" @click="fill">Copy in</button>
          <button type="button" class="ghost" :disabled="busy" @click="readFolder">
            Re-read the folder
          </button>
          <button type="button" class="secondary" :disabled="busy" @click="folderDryRun">
            Dry run
          </button>
          <button
            type="button"
            class="primary"
            :disabled="!canPublishFolder"
            @click="publishTheFolder"
          >
            Publish and empty
          </button>
        </div>

        <p v-if="folderChanged" class="error" role="alert">
          The folder has changed since the dry run — a file was added, removed or edited. Run it
          again: the review has to be of what would actually be published.
        </p>
        <p v-else-if="!folderReviewed" class="muted small">
          Publish is unavailable until a dry run of this folder has been reviewed.
        </p>
      </template>
    </section>

    <!-- The session road: what the specification describes (§6.3). Still here
         while folder publishing is being verified against real photographs. -->
    <section class="road road--older" aria-label="Publish a handed-over session">
      <h2 class="road__head">// A HANDED-OVER SESSION //</h2>
      <p class="muted small">
        The older road: the desktop hands a card to the server, and the session is published from
        here. Unchanged, and still the one the specification describes.
      </p>

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
    </section>

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
.road {
  display: grid;
  gap: var(--space-3);
  border: var(--border-hair);
  background: var(--bg-panel);
  padding: var(--space-3);
}
/* Present, and visibly the secondary of the two. */
.road--older {
  background: transparent;
}
.road__head {
  font-family: var(--font-label);
  font-size: 13px;
  letter-spacing: 0.1em;
  color: var(--accent);
}
.road--older .road__head {
  color: var(--text-muted);
}
.rows {
  border: var(--border-hair);
  background: var(--bg-elevated);
  max-height: 40vh;
  overflow-y: auto;
}
.rows__row {
  display: grid;
  grid-template-columns: 1fr 90px;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  border-bottom: var(--border-hair);
  font-size: 13px;
}
.rows__row:last-child {
  border-bottom: none;
}
.rows__name {
  overflow-wrap: anywhere;
  color: var(--text-heading);
}
.rows__size {
  font-variant-numeric: tabular-nums;
  text-align: right;
  color: var(--text-muted);
}
.skipped {
  list-style: none;
  display: grid;
  gap: var(--space-1);
  font-size: 12px;
  color: var(--text-muted);
}
.note {
  font-size: 13px;
  color: var(--accent);
}
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
