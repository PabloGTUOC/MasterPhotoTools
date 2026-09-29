<script setup lang="ts">
/**
 * Publishing the publishing folder to Google Photos.
 *
 * A web view, and deliberately not a shared one. The Google refresh token lives
 * on exactly one machine (§2.3), so publishing is something only a build
 * talking to the server can do.
 *
 * **One folder, and only that folder.** Copy photographs in, review what would
 * go, publish, and the folder is emptied of everything Google confirmed. The
 * card-handoff session this screen used to publish is gone from it
 * (`docs/workflow-plan.md`); the road it belonged to is retired separately.
 *
 * **Publish is unreachable until a dry run has been reviewed.** That is §9.2
 * rule 3, and it is enforced on the server as well — the button is disabled
 * here because a disabled button is a better explanation than a rejection, not
 * because the server trusts this screen. The review is bound to the exact bytes
 * in the folder, so editing it afterwards un-reviews it.
 */
import { computed, onMounted, onUnmounted, ref } from 'vue';
import type { ConnectorStatus, FolderPublishPlan } from '@phototools/shared';
import JobProgress from '@ui/components/JobProgress.vue';
import PathListField from '@ui/components/PathListField.vue';
import { useRoots } from '@ui/useRoots';
import { api } from '../api';
import { server } from '../api';

const connector = ref<ConnectorStatus | null>(null);
const jobId = ref<string | null>(null);
const busy = ref(false);
const isPublishing = ref(false);
const failure = ref<string | null>(null);
let publishWatcher: AbortController | null = null;

// --- publishing a folder (docs/publish-folder-plan.md) --------------------
//
// The road that lets the tools run before anything is published. The session
// road below it still works and is still the one the specification describes;
// both are here while this one is being verified against real photographs
// (MV-16), and the older one is retired in a change of its own afterwards.

const { roots, failure: rootsError } = useRoots();
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
const reviewedCount = ref<number | null>(null);

const folderChanged = computed(
  () => reviewedSession.value !== null && reviewedSession.value !== folder.value?.session_id,
);

const canPublishFolder = computed(
  () =>
    folderReviewed.value &&
    !folderChanged.value &&
    !busy.value &&
    !isPublishing.value &&
    (folder.value?.items.length ?? 0) > 0 &&
    connector.value?.connected === true,
);

/**
 * Explanations for why publishing is locked.
 *
 * Every applicable condition is reported as a distinct, clear sentence so the
 * user knows exactly what must happen before photographs can be sent to Google.
 * During an active upload, the explanation reports the upload rather than
 * demanding a dry run.
 */
const lockReasons = computed(() => {
  if (isPublishing.value) {
    return ['An upload is in progress.'];
  }
  const reasons: string[] = [];
  if (busy.value) {
    reasons.push('Work is in progress.');
  }
  if (folderChanged.value) {
    reasons.push(
      'The folder has changed since the dry run — run it again so the review matches what will be published.',
    );
  } else if (!folderReviewed.value) {
    reasons.push('Publish is unavailable until a dry run of this folder has been reviewed.');
  }
  if ((folder.value?.items.length ?? 0) === 0) {
    reasons.push('The publishing folder is empty.');
  }
  if (connector.value === null) {
    reasons.push('Checking the Google Photos connection…');
  } else if (!connector.value.connected) {
    reasons.push('Google Photos is not connected.');
  }
  return reasons;
});

const publishButtonLabel = computed(() => {
  if (folderReviewed.value && reviewedCount.value !== null) {
    return `Publish ${reviewedCount.value} and empty`;
  }
  return 'Publish and empty';
});

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
    reviewedSession.value = null;
    reviewedCount.value = null;
    await readFolder();
  }
}

async function folderDryRun() {
  const result = await guard(() => server.planFolderPublish());
  if (result) {
    folder.value = result;
    folderReviewed.value = true;
    reviewedSession.value = result.session_id;
    reviewedCount.value = result.items.length;
  }
}

async function publishTheFolder() {
  // Re-read immediately before publishing. If files were added, deleted or
  // edited outside the browser, the session id changes and publish is aborted.
  await readFolder();
  if (!canPublishFolder.value) {
    return;
  }

  isPublishing.value = true;
  failure.value = null;
  try {
    const id = await guard(() => server.publishFolder());
    if (id) {
      jobId.value = id;
      let streamDropped = false;
      publishWatcher = new AbortController();
      try {
        await api.watchJob(id, () => {}, publishWatcher.signal);
      } catch (e) {
        if ((e as Error).name !== 'AbortError') {
          streamDropped = true;
        }
      } finally {
        publishWatcher = null;
      }
      // One review authorises one publish, and the folder is emptied by a
      // successful one — so whatever is there next is something else.
      folderReviewed.value = false;
      reviewedSession.value = null;
      reviewedCount.value = null;
      await readFolder();
      if (streamDropped && !failure.value) {
        failure.value =
          'The progress stream dropped and the folder was re-read; the result shown may not be final.';
      }
    }
  } finally {
    isPublishing.value = false;
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

function megabytes(bytes: number): string {
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

/**
 * Focus-triggered re-reads are throttled and conditional to protect I/O.
 *
 * GET /api/publish/folder walks the folder and SHA-256 hashes every file
 * (publish/folder.rs) to verify whether content changed. When hundreds of large
 * RAWs or JPEGs reside on a NAS, firing full SHA-256 passes on every tab switch
 * or window focus starts a heavy job.
 *
 * Re-reads on focus run only when a dry-run review actually exists to invalidate
 * (reviewedSession !== null), and at most once every 5 seconds. The re-read
 * immediately before publishing in publishTheFolder stays unconditional.
 */
const FOCUS_THROTTLE_MS = 5000;
let lastFocusCheck = 0;

function onFocus() {
  if (
    document.visibilityState === 'visible' &&
    reviewedSession.value !== null &&
    !isPublishing.value &&
    !busy.value
  ) {
    const now = Date.now();
    if (now - lastFocusCheck >= FOCUS_THROTTLE_MS) {
      lastFocusCheck = now;
      void readFolder();
    }
  }
}

onMounted(async () => {
  window.addEventListener('focus', onFocus);
  document.addEventListener('visibilitychange', onFocus);
  await refreshConnector();
  // The folder is read on opening the tab: whatever is in it is what would be
  // published, however it got there.
  await readFolder();
});

onUnmounted(() => {
  window.removeEventListener('focus', onFocus);
  document.removeEventListener('visibilitychange', onFocus);
  publishWatcher?.abort();
  publishWatcher = null;
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
          :disabled="busy || isPublishing"
          @click="disconnect"
        >
          Disconnect
        </button>
        <button v-else type="button" class="secondary" :disabled="busy || isPublishing" @click="connect">
          {{ connector.needs_reauthorisation ? 'Reconnect' : 'Connect Google Photos' }}
        </button>
      </template>
      <span v-else class="muted small">Checking the connection…</span>
    </section>

    <!-- The folder road. Everything in the publishing folder is uploaded and
         then removed from it; the tools run on the way in. -->
    <section class="road" aria-label="The publishing folder">
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
          :roots-error="rootsError"
          :list="list"
        />

        <p v-if="filled" class="note" role="status">{{ filled }}</p>

        <div class="row">
          <button type="button" class="ghost" :disabled="busy || isPublishing" @click="fill">Copy in</button>
          <button type="button" class="ghost" :disabled="busy || isPublishing" @click="readFolder">
            Re-read the folder
          </button>
          <button type="button" class="secondary" :disabled="busy || isPublishing" @click="folderDryRun">
            Dry run
          </button>
          <button
            type="button"
            class="primary"
            :disabled="!canPublishFolder"
            @click="publishTheFolder"
          >
            {{ publishButtonLabel }}
          </button>
        </div>

        <div
          v-if="lockReasons.length"
          class="gate-explanation"
          data-testid="gate-explanation"
          role="status"
        >
          <p v-for="reason in lockReasons" :key="reason">{{ reason }}</p>
        </div>
      </template>
    </section>

    <p v-if="failure" class="error">{{ failure }}</p>

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
.gate-explanation {
  display: grid;
  gap: var(--space-1);
  padding: 10px 12px;
  border: 1px solid var(--accent-warm);
  border-radius: var(--radius-none);
  background: var(--bg-elevated);
  color: var(--accent-warm);
  font-family: var(--font-body);
  font-size: 13px;
  line-height: 1.4;
}
.gate-explanation p {
  margin: 0;
}
.small { font-size: 13px; }
.mono { font-family: var(--font-body); }
summary { cursor: pointer; font-size: 14px; min-height: 32px; }
</style>
