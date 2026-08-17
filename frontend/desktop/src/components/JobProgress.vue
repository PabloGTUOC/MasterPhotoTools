<script setup lang="ts">
/**
 * Follows a job's SSE stream to its terminal event, with cancel.
 *
 * Cancelling stops watching; the job keeps running on the server, which is what
 * the button says.
 */
import { onUnmounted, ref, watch } from 'vue';
import type { JobEvent } from '@phototools/shared';
import { api } from '../api';

const props = defineProps<{ jobId: string | null }>();

const event = ref<JobEvent | null>(null);
const failure = ref<string | null>(null);
const watching = ref(false);
let controller: AbortController | null = null;

function stop() {
  controller?.abort();
  controller = null;
  watching.value = false;
}

watch(
  () => props.jobId,
  async (id) => {
    stop();
    event.value = null;
    failure.value = null;
    if (!id) return;

    controller = new AbortController();
    watching.value = true;
    try {
      await api.watchJob(id, (next) => (event.value = next), controller.signal);
    } catch (e) {
      if ((e as Error).name !== 'AbortError') {
        failure.value = e instanceof Error ? e.message : String(e);
      }
    } finally {
      watching.value = false;
    }
  },
  { immediate: true },
);

onUnmounted(stop);

const percent = () => {
  const e = event.value;
  if (!e || e.total === 0) return null;
  return Math.min(100, Math.round((e.progress / e.total) * 100));
};
</script>

<template>
  <section v-if="jobId" class="job" aria-live="polite">
    <header class="job-head">
      <span class="job-state" :data-state="event?.state ?? 'pending'">
        {{ event?.state ?? 'starting' }}
      </span>
      <button v-if="watching" type="button" class="ghost" @click="stop">
        Stop watching
      </button>
    </header>

    <div v-if="percent() !== null" class="bar" role="progressbar"
         :aria-valuenow="percent() ?? 0" aria-valuemin="0" aria-valuemax="100">
      <div class="bar-fill" :style="{ width: `${percent()}%` }"></div>
    </div>

    <p v-if="event?.message" class="job-message">{{ event.message }}</p>
    <p v-if="failure" class="error">{{ failure }}</p>
    <p v-if="!watching && !event" class="muted">Waiting for the first update…</p>
  </section>
</template>

<style scoped>
.job {
  border: 1px solid var(--rule);
  border-radius: 10px;
  padding: 14px;
  background: var(--surface-2);
  display: grid;
  gap: 10px;
}
.job-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}
.job-state {
  font-family: var(--mono);
  font-size: 0.8rem;
  text-transform: uppercase;
  letter-spacing: 0.06em;
  padding: 3px 8px;
  border-radius: 999px;
  border: 1px solid var(--rule);
}
.job-state[data-state='completed'] { color: var(--ok); border-color: var(--ok); }
.job-state[data-state='failed'],
.job-state[data-state='interrupted'] { color: var(--critical); border-color: var(--critical); }
.bar {
  height: 6px;
  border-radius: 999px;
  background: var(--rule);
  overflow: hidden;
}
.bar-fill {
  height: 100%;
  background: var(--accent);
  transition: width 200ms ease;
}
.job-message {
  font-family: var(--mono);
  font-size: 0.85rem;
  color: var(--ink-soft);
  word-break: break-word;
}
@media (prefers-reduced-motion: reduce) {
  .bar-fill { transition: none; }
}
</style>
